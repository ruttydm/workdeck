# Wheel-driven review selection

## Full-metadata fixture follow-up

The wheel, page-key, and down-arrow snapshot-publication tests now retain the
pinned fixture's complete 12-line and 50-line sources, original runtime IDs,
default TypeScript language, empty patch field, and both agent annotations.
Embedded sources use `DiffMetadata`: they contribute geometry but do not grant
an implicit fetch capability. The former patch-only fixture omitted these facts.

Each native interaction compares its file projection against frozen data from
both pins in [the mouse-scroll file oracle](oracles/interaction-mouse-scroll-files.json).
The comparison covers source text, change counts, hunk ranges, IDs, language,
annotations, partial status, patch field, and fetcher absence. It does not compare
internal parser caches or claim the whole bootstrap helper is mapped.

The three original interaction tests were rerun on both pins: each run passed
three tests with seven assertions and no failures. Main emitted the existing
`act(...)` environment warning; stable did not. This remains scoped navigation
and fixture evidence, not terminal-cell or release parity.

The first full-suite run passed 1,176 tests but timed out in two pre-existing
queue tests. Those tests started the two-second request deadline before building
their review application. Review construction now precedes dispatch in both
tests; their deadlines and result assertions are unchanged. This separates
fixture initialization from the event-loop behavior being tested.
The rerun passed all 1,178 TUI library tests with no failures or ignored tests
in 15.02 seconds. Formatting and diff checks passed. No new ledger interval is
claimed by this follow-up, and full-workspace/release gates were not rerun.

## Original wheel integration evidence

The pinned AppHost wheel regression exposed missing integration: native wheel
events changed scroll offset but left session selection on the first file.
Both source pins passed; the translated test initially failed.

After a nonzero vertical wheel movement with published geometry, the native
controller now finds the center of the review viewport in its actual rendered
row geometry. It selects the owning visible file and nearest hunk; equal-distance
hunks prefer the later hunk, as in the pinned viewport selector. It publishes
extension selection events without requesting a reveal or moving the keyboard
cursor. Geometry includes current wrapping, gaps and inline notes.

The production snapshot-publisher regression now passes (1 test, 1,132 filtered,
0.82 seconds). TUI all-target Clippy and formatting pass. The full command
`CARGO_INCREMENTAL=0 cargo test -p workdeck-tui --lib` passes all 1,133 tests,
zero failures/ignored/filtered, in 74.42 seconds, including native watcher tests.
This is TUI-library verification, not a full-workspace or release-gate pass.

This is not a benchmark pass or full scroll-source parity: PageUp/PageDown,
scrollbar interaction, cross-platform terminal transport and same-host performance
remain separate validation requirements. The new geometry lookup also needs
measurement against the large-changeset benchmark gate.
