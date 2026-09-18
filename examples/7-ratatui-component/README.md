# 7-ratatui-component

Two minimal native examples that embed Workdeck's public Ratatui diff view directly.

For crate/API details, see [Ratatui component docs](../../docs/ratatui-component.md).

## Run

The examples are library modules because Workdeck ships exactly one executable. Exercise both embedded apps with:

```bash
cargo test -p workdeck-examples ratatui_component
```

Review the example's own source change with:

```bash
workdeck patch examples/7-ratatui-component/change.patch
```

## What it shows

- embedding `render_workdeck_diff_view` inside a normal Ratatui app shell
- building provider-neutral diff metadata from two file snapshots
- parsing raw unified diff text with `create_workdeck_diff_files_from_patch`
- switching between split and stacked layouts with host-owned controls
- a scrollable terminal diff component reusable by other Rust applications

The in-repo modules import `workdeck-tui` by workspace path. Published consumers use the crate released with Workdeck.
