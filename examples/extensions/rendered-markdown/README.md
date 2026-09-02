# Rendered Markdown extension

This optional native extension previews Markdown through Workdeck's file-view protocol. It parses exact new-side source text with `pulldown-cmark` and returns headings, inline formatting, links, lists, quotes, tables, and fenced code as host-owned symbolic rows.

Stage it from this checkout with:

```bash
cargo xtask extension stage-example rendered-markdown
```

Install the staged directory through Workdeck's native extension configuration. Open the extension command menu and choose **Toggle rendered Markdown**, or press `F8`. Raw diff remains the default and fallback.

The preview preserves source-hunk order, exact new-side bindings, hunk navigation, selection highlighting, and note safety. Files with missing or ambiguous source data, binary or oversized inputs, and unterminated fences fall back to the raw reviewer. The Ratatui host owns layout and painting; the extension never receives a renderer or terminal handle.
