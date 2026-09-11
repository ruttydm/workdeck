+++
title = "Ratatui components"
description = "Embed Workdeck's Rust diff renderer in a Ratatui application."
template = "docs.html"
+++

The Workdeck `workdeck-tui` crate exposes the review renderer without requiring
the CLI shell, global keybindings, session broker, or menus. Ratatui's cell
buffer remains the rendering authority; embedding code supplies a terminal and
decides which provider-neutral files to display.

## Install peers

Add the released Workdeck and Ratatui crates to the embedding application's
`Cargo.toml`. There is no npm, Bun, Node.js, OpenTUI, or JavaScript peer:

```bash
cargo add workdeck-tui workdeck-diff workdeck-core ratatui crossterm
```

`workdeck-diff` is required when importing the standalone renderer. The full
`workdeck` binary already includes these crates and should be used when an
application needs the session broker, global commands, or Workdeck's menus.

## Render one file

Build provider-neutral file metadata from two snapshots or from unified patch
text, then render it into a host-owned Ratatui frame:

```rust
use workdeck_diff::{create_workdeck_diff_files_from_patch, parse_diff_from_files};
use workdeck_tui::{render_workdeck_diff_view, DiffLayout, WorkdeckDiffFile};

let metadata = parse_diff_from_files(
    SourceFile::new("before", "export const n = 1;\n", "value.ts"),
    SourceFile::new("after", "export const n = 2;\n", "value.ts"),
    DiffOptions { context: 3, ..Default::default() },
);
let file = WorkdeckDiffFile::new("value", metadata, "value.ts");
render_workdeck_diff_view(frame, &file, DiffLayout::Split { width: 88 });
```

Derive the width from the host layout. The high-level renderer can own a
scrollable review body; lower-level row primitives do not. Rendering never
creates `.agents/workdeck` state and does not start a session daemon.

## Choose the right primitive

- `render_workdeck_diff_view`: batteries-included single-file diff;
- `render_workdeck_diff_body`: one file's body for a host-owned scroll area;
- `render_workdeck_diff_header`: compact file label and stats;
- `render_workdeck_review_stream`: top-to-bottom multi-file stream;
- `render_workdeck_file_nav`: file navigation without outer chrome or scrolling.

Use `create_workdeck_diff_files_from_patch` for unified diff text or
`parse_diff_from_files` for before/after contents. Row internals are deliberately
not public; build on the normalized file model so rendering behavior remains
shared with the Workdeck reviewer. For complete prop tables and runnable native
examples, see [the component crate guide](/docs/reference/ratatui-components/)
and `examples/7-ratatui-component/` in the repository.

Adapted from Hunk's MIT-licensed OpenTUI component guide, Copyright Modem Labs
Inc. The unsupported JavaScript/OpenTUI embedding surface is replaced by the
native Ratatui crate; Workdeck does not publish or execute the original runtime.
