# Ratatui component

`workdeck-tui` exports reusable terminal diff renderers built from the same native review canvas as the Workdeck CLI.

Use `render_workdeck_diff_view` for a batteries-included single-file surface, or compose the lower-level renderers when you want a Workdeck-like review UI without Workdeck's sidebar, menus, global keyboard shortcuts, session broker, or terminal ownership.

## Install

Add the native crate to a Rust application:

```toml
[dependencies]
ratatui = "0.29"
workdeck-tui = "0.1"
```

The component uses provider-neutral models from `workdeck-core` and parsing/layout behavior from `workdeck-diff`. Applications normally need only the public re-exports from `workdeck-tui`.

## Quick start

```rust
use ratatui::{buffer::Buffer, layout::Rect};
use workdeck_tui::{
    WorkdeckDiffLayout, WorkdeckDiffViewOptions, WorkdeckFileComparisonOptions,
    WorkdeckFileSnapshot, diff_from_workdeck_file_snapshots,
    render_workdeck_diff_view,
};

let file = diff_from_workdeck_file_snapshots(
    WorkdeckFileSnapshot {
        cache_key: "before",
        contents: "pub const VALUE: u8 = 1;\n",
        name: "example.rs",
    },
    WorkdeckFileSnapshot {
        cache_key: "after",
        contents: "pub const VALUE: u8 = 2;\npub const ADDED: bool = true;\n",
        name: "example.rs",
    },
    WorkdeckFileComparisonOptions { context_radius: 3 },
)?;

let area = Rect::new(0, 0, 88, 24);
let mut buffer = Buffer::empty(area);
render_workdeck_diff_view(
    area,
    &mut buffer,
    Some(&file),
    &WorkdeckDiffViewOptions {
        body: workdeck_tui::WorkdeckDiffBodyOptions {
            layout: WorkdeckDiffLayout::Split,
            theme: "midnight".into(),
            ..Default::default()
        },
        ..Default::default()
    },
);
# Ok::<(), workdeck_diff::PatchError>(())
```

In a real app, derive the `Rect` from the host's Ratatui layout. The host owns the terminal backend, draw loop, input routing, and shutdown.

## Convenience versus primitives

### `render_workdeck_diff_view`

This renders one file with caller-controlled vertical scrolling:

```rust
let options = WorkdeckDiffViewOptions {
    scrollable: true,
    vertical_offset: scroll_row,
    ..Default::default()
};
let map = render_workdeck_diff_view(area, buffer, Some(file), &options);
```

Use it when you want a drop-in diff viewport. Its `WorkdeckDiffRenderMap` reports visible hunk rows for host-owned navigation and input.

### `render_workdeck_diff_body`

This renders only the diff body for one file. It does not own scrolling, file navigation, keyboard shortcuts, menus, or session behavior:

```rust
let map = render_workdeck_diff_body(
    area,
    buffer,
    Some(file),
    &WorkdeckDiffBodyOptions {
        layout: WorkdeckDiffLayout::Stack,
        selected_hunk_index: Some(2),
        ..Default::default()
    },
);
```

Use it when the host owns clipping or surrounding layout.

### `render_workdeck_diff_file_header`

This renders Workdeck's compact file label and statistics header:

```rust
let click_target = render_workdeck_diff_file_header(
    area,
    buffer,
    file,
    &WorkdeckDiffFileHeaderOptions {
        selected: true,
        ..Default::default()
    },
);
```

The returned rectangle is the host's click target for file selection.

### `render_workdeck_review_stream`

This renders a top-to-bottom multi-file stream without Workdeck's app shell, chrome, keybindings, or scroll owner:

```rust
let map = render_workdeck_review_stream(
    area,
    buffer,
    files,
    &WorkdeckReviewStreamOptions {
        selection: Some(WorkdeckDiffSelection {
            file_id: file_id.clone(),
            hunk_index,
        }),
        vertical_offset: scroll_row,
        ..Default::default()
    },
);
```

Use the returned file and hunk rows to implement host-specific mouse, keyboard, and selection behavior.

### `render_workdeck_file_nav`

This renders Workdeck's adaptive flat/tree file navigator. It does not render borders, outer padding, or own a scroll area:

```rust
let map = render_workdeck_file_nav(
    area,
    buffer,
    files,
    &WorkdeckFileNavOptions {
        selected_file_id: Some(file_id.clone()),
        ..Default::default()
    },
);
if let Some(next_file_id) = workdeck_file_nav_selection_at(&map, clicked_row) {
    file_id = next_file_id.to_owned();
}
```

For a host-owned scroll viewport, call `render_workdeck_file_nav_window` with a fixed-row
`scroll_top`. The returned hit rows are translated into viewport coordinates. The shipped shell
uses the same renderer behind the process-cached `workdeck:files` bundled pane: its responsive
width is preferred 34, minimum 22, maximum 56, and 16% of the available terminal width.

## Building file inputs

The public model is the provider-neutral `workdeck_core::DiffFile`. It carries:

- stable file and review addresses;
- current and previous paths;
- change kind, binary/large/untracked flags, and addition/deletion statistics;
- parsed hunks and source lines;
- optional before/after source snapshots;
- optional agent annotations.

Internal Ratatui row-planning models are not part of the public file contract. Use `create_workdeck_diff_file` to refresh a constructed file's stable identity and `count_workdeck_diff_stats` to derive visible additions/deletions.

### From before/after contents

Use `diff_from_workdeck_file_snapshots` when the host already has both file contents:

```rust
let file = diff_from_workdeck_file_snapshots(
    WorkdeckFileSnapshot {
        cache_key: "old-object-id",
        contents: before,
        name: "src/lib.rs",
    },
    WorkdeckFileSnapshot {
        cache_key: "new-object-id",
        contents: after,
        name: "src/lib.rs",
    },
    WorkdeckFileComparisonOptions::default(),
)?;
```

Cache keys participate in stable source identity. They must change when the corresponding contents change.

### From unified diff text

Use `create_workdeck_diff_files_from_patch` for a multi-file unified patch:

```rust
let files = create_workdeck_diff_files_from_patch(patch_text, "example:patch")?;
```

The parser accepts the same provider-neutral patch model as the Workdeck CLI.

## Common options

| Field | Type | Default | Notes |
| --- | --- | --- | --- |
| `layout` | `WorkdeckDiffLayout` | `Split` | Chooses split or stacked rendering; the wider shell may also resolve `Auto`. |
| `theme` | `String` | `github-dark-default` | Resolves a bundled Workdeck/Shiki-compatible theme name. |
| `show_line_numbers` | `bool` | `true` | Toggles line-number columns. |
| `show_hunk_headers` | `bool` | `true` | Toggles `@@ ... @@` rows. |
| `tab_width` | `u16` | `4` | Sets source-code tab stops. Workdeck CLI validation accepts 1 through 16. |
| `hunk_gap` | `u16` | `0` | Adds blank rows before hunks after the first. |
| `wrap_lines` | `bool` | `false` | Wraps long lines instead of clipping horizontally. |
| `horizontal_offset` | `usize` | `0` | Scroll offset for non-wrapped code rows. |
| `highlight` | `bool` | `true` | Enables syntax highlighting. |
| `selected_hunk_index` | `Option<usize>` | `Some(0)` | Marks one hunk as the active target. |
| `scrollable` | `bool` | `true` | View-only switch controlling whether `vertical_offset` is applied. |
| `vertical_offset` | `usize` | `0` | Host-owned top row for a view or review stream. |
| `file_gap` | `u16` | `1` | Review-stream rows between files, including the separator row. |
| `show_file_separators` | `bool` | `true` | Toggles separator rules between files. |

## Other exports

- `diff_from_workdeck_file_snapshots`
- `create_workdeck_diff_file`
- `create_workdeck_diff_files_from_patch`
- `count_workdeck_diff_stats`
- `WORKDECK_DIFF_THEME_NAMES`
- `WorkdeckDiffThemeName`
- `WorkdeckDiffLayout`
- `WorkdeckDiffFile`
- `WorkdeckDiffFileInput`
- `WorkdeckDiffStats`
- `WorkdeckDiffSelection`
- body, view, header, stream, and file-navigation option and render-map types

## Examples

- Demo overview: [`examples/README.md`](../examples/README.md)
- Native component demos: [`examples/7-ratatui-component/README.md`](../examples/7-ratatui-component/README.md)

The in-repo examples import by workspace path so they run from source. Published consumers import the released `workdeck-tui` crate.
