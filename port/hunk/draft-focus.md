# Draft presence and input focus

The pinned AppHost blur regression (source lines 3145–3186) exposed an actual
native mismatch: every mouse event was consumed while a draft existed. Clicking
outside left keyboard input captured, and `s` appended text instead of toggling
the sidebar. Both unchanged Hunk baselines passed; the initial Rust test failed.

`ReviewNoteComposer` now carries private focus state. Create/edit/reply drafts
start focused. Outside left clicks release focus and continue through normal
mouse routing; clicks inside refocus the draft. Draft existence continues to
control its rendering and file-view restrictions. Focus controls keyboard and
paste ownership, menu-toggle suppression, editor caret and inline presentation.
No persisted Workdeck schema changes.

The original regression passes after the fix. Expanded assertions cover ignored
paste and hidden caret while blurred, then restored typing/paste after refocus.
Formatting and TUI all-target Clippy pass. `CARGO_INCREMENTAL=0 cargo test -p
workdeck-tui --lib` passes all 1,127 tests, zero failed/ignored/filtered, in 86.22
seconds, including all three native watcher tests and the expanded focus test.
This verifies the changed TUI library, not full-workspace or cross-platform parity.
