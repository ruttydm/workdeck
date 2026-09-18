+++
title = "File previews"
description = "Add opt-in file presentations that keep Workdeck review navigation, scrolling, and inline notes."
template = "docs.html"
+++

`workdeck-extension-api` lets a native extension offer a different way to read a
changed file. A Markdown extension can render headings and lists, a package
extension can summarize dependency changes, and a CSS extension can put color
swatches beside changed values.

A preview remains part of Workdeck's normal review stream. Workdeck owns file
ordering, measurement, scrolling, windowing, hunk navigation, selection, and
inline notes. The extension describes deterministic rows; it does not replace
the review pane. The API is versioned and opt-in: installing an extension never
silently changes a file's presentation.

## What users see

Raw diff is always the default. When a registered view matches the selected
file, Workdeck adds it under **View → File presentation**. The user can choose a
presentation for each file independently, so one review may contain custom
previews and ordinary diffs together.

**View → Apply “…” to all matching files** selects the view for every matching
file in the changeset, including files hidden by the current filter. Files that
do not match keep their existing presentation. An extension command may select
or toggle its view for the current file:

```rust
api.register_command(Command {
    id: "toggle-preview".into(),
    title: "Toggle preview".into(),
    key: Some(Key::F8),
    handler: |ctx| {
        ctx.file_views().toggle("preview")
    },
});
```

`select("preview")` selects a view, `select(None)` returns to raw diff, and
`is_active("preview")` reports the current selection. A bare ID names the
calling extension's view; `other-extension:preview` addresses another
registered view. These controls target only the current file; applying a view
across the changeset remains a host-owned View-menu action.

`refresh("preview")` invalidates prepared layouts. Workdeck treats layout as a
pure derivation of `(file, width)` and reuses it until one changes. A view that
holds state (a fold or per-file overlay) flips that state and requests a
re-derivation. Refresh defaults to view-wide: every file presenting the view
re-runs `matches` and `layout`; raw-diff files do no work. Existing rows remain
visible until replacement resolves, so refresh never flashes back to raw diff.
Pass `{ file_id }` for a file-local refresh; an unknown ID invalidates nothing.

## Register a view

A view has an ID, title, cheap matcher, and deterministic layout function:

```rust
api.register_file_view(FileView {
    id: "preview".into(),
    title: "Line preview".into(),
    matches: |file| file.path.ends_with(".md"),
    layout: |input| async move {
        let Some(document) = input.read_document(SourceSide::New).await? else {
            return Ok(None);
        };
        if document.is_empty() { return Ok(None); }
        let lines = document.trim_end_matches('\n').split('\n').collect::<Vec<_>>();
        let mut hunk_rows = Vec::new();
        for hunk in input.file.hunks() {
            let Some((start, end)) = hunk.new_range() else { return Ok(None); };
            if start == 0 || end < start { return Ok(None); }
            hunk_rows.push(RowExtent {
                start: (start - 1).min(lines.len() - 1),
                end: (end - 1).min(lines.len() - 1),
            });
        }
        Ok(Some(Layout {
            rows: lines.into_iter().enumerate().map(|(index, text)| Row {
                id: format!("line:{}", index + 1),
                spans: vec![Span::text(if text.is_empty() { " " } else { text })],
                ..Default::default()
            }).collect(),
            hunk_rows,
        }))
    },
});
```

Return `None` whenever the view cannot safely present a file. Workdeck keeps or
restores raw diff. This smallest example omits source bindings; add them only
to rows owned by exactly one hunk extent, as described below.

### Matching

`matches(file)` decides whether the view appears in the View menu. Keep it fast,
side-effect free, and based on `file.path`, `file.change_type`,
`file.is_binary`, or `file.is_too_large`. If it fails, Workdeck excludes the
view for that file and keeps raw diff.

### Layout input

`layout(input)` receives one immutable snapshot:

| Field | Meaning |
| --- | --- |
| `file` | Public file model: path, change type, stats, patch text, and ordered hunk summaries. |
| `width` | Available terminal columns. The same input and width must produce the same layout. |
| `cancel` | Aborts when reload, resize, selection, view, or extension reload supersedes the request. |
| `changes` | Typed added/removed source ranges with hunk indexes. |
| `read_document(side)` | Lazily reads the exact old or new source document. |

`read_document` resolves to a string, an empty string for a valid empty
document, or `None` when the side is absent, unreadable, or exceeds host
limits. Cancellation aborts the pending request. Patch text is available on
`input.file.patch`, but it is not a complete source document. Reads are lazy and
deduplicated within a request.

### Layout output

A layout contains `rows` and `hunk_rows`. Every row has a stable ID and a
symbolic `spans` array. Spans contain text plus an optional semantic tone and
terminal attributes:

```rust
Row {
    id: "dependency:react".into(),
    spans: vec![
        Span::new("react").with_attributes([Attribute::Bold]),
        Span::new(" 19.1.0 → 19.2.0").with_tone(Tone::Added),
    ],
    ..Default::default()
}
```

Tones are `muted`, `accent`, `accent-muted`, `syntax`, `added`, and `removed`.
Attributes are `bold`, `italic`, `underline`, and `strikethrough`. Span text
cannot contain newlines; create one row per line. Workdeck maps tones to the
active theme while painting, so changing themes does not require a new layout.

`hunk_rows` has one inclusive, zero-based extent for every item in
`input.file.hunks`, in the same order. Workdeck uses these extents for `[`/`]`
navigation and selected-hunk highlighting even when several preview rows
represent one source hunk.

## Keep inline notes attached to source

A row may declare the exact old/new source lines it presents:

```rust
Row {
    id: "rendered-paragraph:4".into(),
    spans: vec![Span::text("A rendered paragraph")],
    source_ranges: vec![SourceRange::inclusive(SourceSide::New, 12, 15)],
    ..Default::default()
}
```

Source ranges are inclusive and one-based. Workdeck verifies that they exist in
the exact source document, do not overlap ranges owned by another row on the
same side, and belong to one hunk extent.

When agent notes are visible, Workdeck inserts its own note cards before the
matching preview row. The extension never receives note contents and never
measures note UI. Placement is all-or-raw for each file: if any visible note has
no unique bound row, Workdeck temporarily shows the complete raw diff rather
than hiding the note or guessing. The selected preview returns when notes are
hidden or all bindings resolve. Draft note editing remains on raw diff. Omit
`source_ranges` when a preview does not support note placement; it still works
when no visible note needs a binding.

## Paint a fixed-height JSX row (native)

A row may replace symbolic paint with a constrained declarative `component`:

```rust
Row {
    id: "package-summary".into(),
    spans: vec![Span::text("Package changes").bold()],
    component: Some(Component {
        height: 2,
        content: ViewNode::column([
            ViewNode::text("Package changes"),
            ViewNode::text("Selected rows retain their semantic theme"),
        ]),
        selected_content: None,
    }),
    ..Default::default()
}
```

The declared height is final. Workdeck measures and windows the review before
painting, then clips each component to its assigned rectangle. Components
cannot grow a row, replace the scrollbox, or perform post-paint measurement.
Painter data contains only fixed geometry, selected-hunk state, row position,
and a live semantic theme. Theme changes repaint without rerunning layout.

Always provide useful spans. If a declarative tree is invalid, Workdeck paints
those spans inside the same fixed height. Components are non-focusable paint
surfaces; registered commands are the keyboard path. Mouse delivery is
cooperative, while wheel scrolling, dragging, and unhandled input remain
host-owned. Component state is ephemeral and is reset by windowing unmounts,
width/layout generations, presentation changes, or reloads. Durable extension
state belongs outside the row.

## Interactive previews

Add a synchronous keyboard mode when a preview needs input:

```rust
mode: Some(FileViewMode {
    on_key: |key, ctx| {
        if key.is_space() {
            ctx.file_views().refresh("preview", None);
            KeyResult::Handled
        } else {
            KeyResult::Pass
        }
    },
    ..Default::default()
}),
```

Start it with `enter_mode("preview")`; entering also selects the preview and
returns whether the mode started. Only one interactive preview mode runs at a
time. `exit_mode()` stops it and `is_mode_active("preview")` checks it. It may
overlap a session keyboard mode but receives keys first until it exits.

`Handled` consumes a key, `Pass` continues through an active session mode and
normal Workdeck routing, and `Exit` consumes the key and stops the mode. The
callback must return synchronously. Escape is reserved by Workdeck while the
file-view mode owns input. Modes exit when their file, presentation, extension,
or review session changes. Optional enter/exit callbacks run synchronously;
exit runs exactly once per activation. A failing callback exits the mode and
warns without breaking the review.

## Validation and fallback

Workdeck validates every returned layout before using it. Current limits are:

| Limit | Maximum |
| --- | ---: |
| Rows | 10,000 |
| Spans | 40,000 |
| Source bindings | 40,000 |
| Symbolic text | 1,000,000 characters |
| One component row | 256 terminal rows |
| Complete layout | 100,000 terminal rows |
| Layout request | 1.5 seconds |

Layout work runs with bounded concurrency and cached results. Workdeck discards
layouts prepared for an old width, file snapshot, registration, or cancelled
request. A `None`, invalid, oversized, cancelled, timed-out, or throwing layout
produces raw diff; a component failure affects only that row. Native extensions
are trusted programs rather than sandboxes, but fallback remains part of the
normal contract.

## Examples

- The compiled `examples/extensions/rendered-markdown/` fixture is a symbolic-row preview with exact-source bindings and inline notes.
- `examples/extensions/file-view-gallery/` demonstrates fixed-height declarative rows, CSS color swatches, package dependency versions, and mixed raw/custom reviews.
- `examples/extensions/inline-edit/` exercises interactive modes, scoped refresh, consented writes, and stale-source rejection.

The examples are not bundled or loaded by default. Run one directly while
developing:

```bash
cargo run -p rendered-markdown-extension -- \
  --workdeck-diff ./examples/extensions/file-view-gallery/fixtures/before/README.md \
  ./examples/extensions/file-view-gallery/fixtures/after/README.md
```

For installation, discovery, folder extensions, and trust, start with
[Extensions](/docs/extend/extensions/). For the rest of the native API surface,
see [Extension API](/docs/extend/extension-api/).

Adapted from Hunk's MIT-licensed file-preview guide, Copyright Modem Labs Inc.
The native page retains the documented row, source-binding, note, mode,
validation, and fallback behavior while replacing React/OpenTUI with Ratatui
and the Workdeck Rust SDK.
