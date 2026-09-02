# Fixed-height file-view component example

This opt-in native extension is the Rust/Ratatui port of Hunk's JSX file-view proof of concept. It appears only for files with at least two parsed hunks and creates two custom rows per hunk, with stable row IDs and explicit hunk bounds.

Stage it from this checkout and open a multi-hunk working-tree change:

```bash
cargo xtask extension stage-example jsx-file-view
workdeck diff --extension target/workdeck-extension-examples/jsx-file-view
```

Choose **Extensions → Toggle JSX hunk cards (POC)**. F8 is the registered keyboard path. Each two-line row has host-owned ephemeral expanded state: an un-dragged left-button mouse-up toggles its detail, while wheel, drag, and other input remain owned by the Ratatui review shell. Stable IDs keep state across selected-hunk and theme paints; state is intentionally lost when row windowing unmounts the row, the width changes, the view switches, or a new layout generation replaces it. Symbolic spans remain the clipped fallback for an empty accepted component tree; invalid layouts use the raw diff.

The subprocess never receives terminal ownership and does not load React or OpenTUI. See [`docs/file-view-native-components.md`](../../../docs/file-view-native-components.md) for the translated contract and limits.
