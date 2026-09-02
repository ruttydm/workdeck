# Mixed preview review

This launches one real five-file Git working-tree review that is intentionally taller than a
terminal viewport:

- `README.md` — raw Markdown diff;
- `Cargo.toml` — dependency version-segment highlights;
- `scripts/deploy.py` — raw Python diff;
- `src/invoice.rs` — responsive change-atlas cards;
- `styles/theme.css` — exact-source color swatches.

The Rust launcher creates a temporary repository, commits the checked-in `before` fixtures, copies
the `after` fixtures into its working tree, and starts Workdeck from this checkout. The temporary
repository is removed when Workdeck exits.

From the Workdeck repository root:

```console
cargo xtask extension stage-example file-view-gallery
cargo run -p workdeck-examples --bin workdeck-example-file-view-gallery-mixed-review
```

Raw diff is deliberately the default. To build the mixed stream:

1. Select `Cargo.toml` in the sidebar and press **F8**.
2. Select `src/invoice.rs` and press **F8**.
3. Select `styles/theme.css` and press **F8**.
4. Select `README.md` to return to the top, then scroll through the main pane.

Selecting files only jumps the main review stream; it does not collapse other files. The three
preview selections therefore remain active together, interleaved with the two raw Ratatui diffs.
Use `n` and `p` while scrolling to verify that hunk navigation crosses raw and custom sections using
the same host-owned geometry.
