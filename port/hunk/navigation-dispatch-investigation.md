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
