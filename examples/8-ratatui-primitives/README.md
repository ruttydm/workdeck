# 8-ratatui-primitives

A small custom Ratatui app assembled from Workdeck's lower-level review primitives instead of the full CLI shell.

For crate/API details, see [Ratatui component docs](../../docs/ratatui-component.md).

## Run

The demo is a library module because Workdeck ships exactly one executable. Exercise the composed UI with:

```bash
cargo test -p workdeck-examples ratatui_primitives
```

## What it shows

- `create_workdeck_diff_files_from_patch` for turning unified Rust diff text into public file models
- `render_workdeck_file_nav` for a standalone adaptive file list
- `render_workdeck_review_stream` for a multi-file stream without Workdeck's menu bar or global shortcuts
- `render_workdeck_diff_file_header` and `render_workdeck_diff_body` for a single-file view assembled by the host
- host-owned borders and chrome around each primitive so component boundaries remain visible
- host-owned state for selected file, split/stack layout, next-file navigation, and quit behavior

The in-repo demo imports `workdeck-tui` by workspace path. Published consumers use the crate released with Workdeck.
