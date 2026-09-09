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
