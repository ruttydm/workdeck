+++
title = "Comments and annotations"
description = "Attach human or agent review notes to code and navigate them in context."
template = "docs.html"
+++

Notes are hunk-specific and render beside the rows they explain. Workdeck intentionally keeps them in the review flow rather than in a separate comments screen.

## Add one agent comment

```bash
workdeck session comment add \
  --repo . \
  --file README.md \
  --new-line 103 \
  --summary "Tighten this wording"
```

Choose exactly one `--old-line` or `--new-line` target. Add `--focus` only when the new note should move the user's viewport.

## Apply a batch

```bash
printf '%s\n' '{"comments":[{"filePath":"README.md","newLine":103,"summary":"Tighten this wording"}]}' \
  | workdeck session comment apply --repo . --stdin
```

Every item needs `filePath`, `summary`, and exactly one target: `hunk`, `hunkNumber`, `oldLine`, or `newLine`. Workdeck validates the complete batch before changing the live session.

## Inspect and clean up

```bash
workdeck session comment list --repo .
workdeck session comment list --repo . --type all
workdeck session comment rm --repo . <comment-id>
workdeck session comment clear --repo . --file README.md --yes
```

Use `--all --yes` to clear both live agent comments and human notes. Destructive clears require confirmation.

## Add a human note

In the TUI, select a hunk and press `c` or click an add-note affordance. Human and agent notes are labeled by source. Use `{` and `}` to move through annotated hunks across the review stream.

The examples add or remove data from the selected live review only when you run
them. Inspect the session and note scope before clearing; `--all` includes human
notes. Static sidecar annotations are a separate input source, not a reason to
delete the original sidecar file.

Adapted from Hunk's pinned MIT guide, Copyright Modem Labs Inc. Full note,
annotation and lifecycle parity remains unverified; this source stays unmapped.

