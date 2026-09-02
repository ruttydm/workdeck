# Pane layout extension

This compiled Rust extension registers a resizable right pane and fixed two-row top and bottom panes. All three begin closed, matching Hunk’s example.

Stage the executable and manifest from this checkout, then run it explicitly:

```bash
cargo xtask extension stage-example pane-layout
cargo run -p workdeck-cli -- --extension ./target/workdeck-extension-examples/pane-layout diff
```

Press `ctrl+p` to open or close all three panes. Drag the right divider to resize its pane. The host retains terminal ownership and passes each declarative pane its exact allocated width, height, placement, selected review file, and paint-only theme.
