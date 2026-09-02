# 9 — agent markup notes (STML)

Shows agent notes carrying **STML markup**: small HTML-like markup that Workdeck renders as real terminal UI inside an inline note card. It supports bordered boxes, rows of shapes, lists, badges, and code blocks instead of plain text.

Run from the repository root:

```sh
workdeck patch examples/9-agent-markup-notes/change.patch \
  --agent-context examples/9-agent-markup-notes/agent-context.json
```

Press `a` to reveal agent notes for the selected hunk.

The same markup works for live comments from an agent driving a session:

```sh
workdeck session comment add --repo . --file src/retry.rs --new-line 3 \
  --summary "Retry flow" \
  --markup '<box border border-color="accent">shapes in a note</box>' \
  --focus
```

Learn and iterate from the CLI:

```sh
workdeck markup guide
echo '<badge color="success">OK</badge> ready' | \
  workdeck markup render - --width 56
```

Block tags are `box`, `card`, `row`, `text`, `h1`–`h3`, `list`/`item`, `hr`, `spacer`, and `code`. Inline tags are `b`, `i`, `u`, `s`, `dim`, `color`, `kbd`, `badge`, `a`, and `br`. Colors accept semantic tokens (`accent`, `success`, `warning`, `danger`, `info`, `muted`), ANSI-style names, or hex.

The retry pair, patch, sidecar, and both rich notes are executable parity tests in `workdeck-examples`.
