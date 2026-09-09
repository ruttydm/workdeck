# Navigation dispatch investigation

At `c150c3b8`, the diagnostic captured at `e10dafb1` reports a 225.53 ms
median navigation press, including 179.24 ms median dispatch, in an unoptimized
build. See [raw sample](interaction-diagnostic-e10dafb1.json). This single
sample is not a release benchmark or an attribution of time to individual calls.

Inspection of `crates/workdeck-tui/src/lib.rs` identifies these candidates:

- `move_selection` constructs a semantic navigation model and reveals selection.
- `scroll_to_reveal` constructs geometry rows with syntax highlighting disabled.
- `commit_extension_runtime_bridge` projects every diff file and constructs a
  review snapshot before committing selection, commands and generation authority.
- Selection publication commits the bridge; individual lifecycle event publication
  also commits it, potentially repeating unchanged document projection work.

Do not skip bridge commits merely because there are no event subscribers.
`publish_extension_selection_events` explicitly preserves retained controls'
authority even without subscribers. The regression
`review_app_commits_extension_runtime_authority_across_selection_reload_and_registry_change`
passes at `c150c3b8` using
`CARGO_INCREMENTAL=0 cargo test -p workdeck-tui --lib review_app_commits_extension_runtime_authority`.
It checks live selection and control retirement across reload/registry changes.

Next investigate reuse of immutable document projections independently from
selection/command/generation publication. Any cache must invalidate on document
replacement and on metadata changes represented in the extension file view.
Measure individual stages before attributing the observed dispatch latency to
projection work. No runtime optimization or completed ledger mapping is claimed
by this investigation.

## Implemented projection reuse

The follow-up caches file projections by the retained immutable changeset Arc,
sharing the projection slice between bridge commits. Public getters still
return owned values; selection, review snapshots, commands and generation
authority are freshly committed. Equal-content replacement documents invalidate
the cache, as do replacements with metadata changes.

Ten focused runtime-bridge tests and the new exact-document cache test pass,
along with scoped Clippy and formatting. The follow-up debug diagnostic reports
about 99.08 ms median navigation and 50.91 ms dispatch, versus 225.53 ms and
179.24 ms in the earlier sample. See [raw follow-up and source hashes](interaction-diagnostic-projection-cache.json).
These single samples support the optimization but are not a controlled paired
release benchmark, an isolated stage profile, or proof of the Hunk 10% gate.

Ownership regressions additionally mutate paths, nested JSON metadata and hunk
headers returned from bridge getters, checking that committed and preview
projections remain unchanged. A weak-reference assertion verifies that cache
replacement releases the old document, while retained projected values remain
readable. Shared internal storage does not change the owned-value getter boundary.

The complete TUI library suite also passed after these changes: 1,101 tests,
zero failures, ignored tests or filters on Darwin arm64 at `afbe27fb`.
See [verification record](tui-verification-afbe27fb.json). This expands native
regression coverage beyond the focused bridge tests, not the release-gate claim.

The interaction diagnostic now reports `peakProcessRssBytes` separately from
its current memory snapshots. This is the process-lifetime high-water mark,
including construction of both navigation and scrolling fixtures; it is not a
per-stage peak or JavaScript heap measurement. The executable interaction test
checks the field is positive on supported native hosts. Earlier captured JSON
reports remain unchanged and must not be interpreted as containing peak data.

## Shared internal review snapshots

The bridge now retains the review state's immutable changeset Arc together with
fresh generation and selection values, rather than deep-copying the document at
startup and every bridge commit. Selection projection accepts a borrowed document;
the existing owned-snapshot helper delegates to the same implementation.
`committed_review` still materializes an owned `ReviewSnapshot` for consumers, so
the public schema and detached-mutation contract are unchanged. All authority
commits remain in place. The regression
`shared_runtime_snapshot_retains_document_but_public_reads_are_detached` checks
shared internal identity and independent public reads. No measured performance
improvement or additional completed ledger interval is claimed by this change.

Validation: 12 focused TUI runtime tests, eight host selection tests, and the
selected-file/full-projection equivalence regression pass. The latter covers
duplicate IDs and absent selections through the borrowed-document helper.
Scoped TUI/extension-host all-target Clippy, formatting and diff checks pass.

The full huge diagnostic at clean `08e77400` completed successfully:
[raw sample](huge-stream-native-08e77400.json). First-frame allocator usage was
558,123,552 bytes versus 633,253,392 in the preceding peak diagnostic, but peak
RSS was 1,393,901,568 versus 1,385,152,512 bytes: this sample does **not** show a
resident-memory improvement. Navigation median was about 705.65 ms versus
733.79 ms; first-frame time remained about 1,133 ms. These single-run debug
observations do not establish causality or pass the release benchmark gate.
Further investigation must include initialization/publication allocations and
allocator retention rather than equating reduced live allocations with peak RSS.

## Borrowed initialization input

`ReviewApp::new_with_extensions` now uses `ReviewProducer::from_files` with a
borrowed file slice and source label. Previously it cloned the entire file vector
into `PublishReviewInput`, only for `ReviewProducer::new` to borrow that temporary
while constructing its separately owned publication. The original owned-input
constructor remains available and delegates to the same builder. Publication,
resource-store and session ownership are unchanged; this removes only the
temporary input copy. The new regression compares owned/borrowed documents and
reads patch and source resources after mutating and clearing the original input.
All 11 producer tests and the TUI authority regression pass, as do scoped
review/TUI all-target Clippy, formatting and diff checks. Peak-memory benefit
was subsequently sampled at clean `7abe958b`: [raw diagnostic](huge-stream-native-7abe958b.json).
Peak RSS was 1,316,552,704 bytes versus 1,393,901,568 in the preceding sample,
about 77 MB lower. First-frame time remained about 1,133 ms and navigation
median about 706 ms. This single debug run is not a controlled attribution or
release gate pass; peak RSS still exceeds both retained pinned-source samples.

The next startup change seeds `ExtensionFileProjectionCache` with the initial
shared document before constructing the first bridge commit, and moves that
same cache into `ReviewApp`. Previously the initial projection was built outside
the cache, so the first authority commit built a second projection while the
first was still retained. Exact-document invalidation and owned public getters
are unchanged. This follow-up has no separate memory measurement yet.
The exact-document cache and TUI reload/authority regressions pass, along with
TUI all-target Clippy, formatting and diff checks.

The complete TUI library suite subsequently passed at clean `b0322ed5`:
1,102 passed, zero failed, ignored or filtered, in 50.93 seconds on Darwin arm64.
See [verification record](tui-verification-b0322ed5.json). This checks the combined
runtime-storage and startup changes, but does not substitute for full-workspace,
cross-platform, source-ledger or benchmark acceptance gates.
