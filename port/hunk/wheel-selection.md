# Wheel-driven review selection

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
