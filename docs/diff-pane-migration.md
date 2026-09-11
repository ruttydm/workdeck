# Hunk `DiffPane` migration

The pinned continuous review pane is `src/ui/components/panes/DiffPane.tsx`. The baseline
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` blob is 106,437 bytes (SHA-256
`2f3c6f032b94f96f8d176def74821336bf96d1c876d5d942141cbe18a83b5f19`, 2,835 lines). The
stable-v0.20.1 blob is 96,299 bytes (SHA-256
`d1c6dca60aec1b1a7aa1e0228e5b5280550fb55e1738b7af7ee35c7d89309d0a`, 2,557 lines). Both blobs
are read directly from their preserved Git refs; neither is copied into the Workdeck tree.

The native projection is an explicit, non-overlapping contract map in `xtask/src/diff_pane.rs`.
It accounts for note metadata/actions, scroll clamping and reveal, stream row bounds, adjacent and
viewport highlight prefetch, the stable initial-viewport estimate, and persistent copy-gesture
capture. The `DiffPane` render contract is split across `render_review`, Ratatui section geometry
and row painting, sparse file windowing, inline-note planning, line cursors, viewport anchors,
selection-follow policy, copy selection, and the public review stream. Every contract has an
executable Rust test anchor and a frozen oracle for both pinned trees.

The copy model keeps a distinct `pinned-header` target alongside review-row targets, so selecting
the pinned header cannot accidentally address the following body row.

The Ratatui shell keeps the pinned file header in a dedicated top row, preserves the review stream
extent with spacers while windowing files, renders all mounted note rows, keeps copy selection in
terminal-cell coordinates, and lets explicit line/file/note reveals supersede passive viewport
selection. Horizontal wheel gestures remain isolated from vertical scroll and wrapped mode keeps
them available for vertical review movement. Stable's former OpenTUI capture and first-viewport
helpers are represented by Workdeck's owned `MouseCapture` and measured viewport policies.

Run the focused gate with:

```text
cargo test --locked -p xtask native_ratatui_review_pane_replaces_both_pinned_source_contracts -- --nocapture
```

The strict port audit invokes this verifier before accepting the source disposition. The native
implementation contains no TypeScript source mirror, JavaScript runtime, or `hunk` executable.
No TypeScript source mirror is committed or executed.
