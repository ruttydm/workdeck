+++
title = "Quick start"
description = "Open a working tree or commit in Workdeck's continuous review canvas."
template = "docs.html"
+++

Workdeck's Review pane presents files in a continuous stream. The sidebar is an
index into that stream rather than a request to hide every other file. Complete
Hunk parity is still under verification; this guide describes the native review
workflow, not a completed parity certification.

## Review current work

From a repository:

```sh
workdeck diff
```

Tracked changes and untracked files are included. Add `--exclude-untracked` when
you want tracked changes only. This opens the reviewer; Workdeck's existing
`workdeck changes diff` remains a separate headless command.

In the review canvas, the default bindings are:

1. `]` jumps to the next hunk.
2. `.` jumps to the next file.
3. `1`, `2` and `0` select split, stack and automatic layout.
4. `q` exits the reviewer.

Selecting a file in the sidebar navigates to that file in the stream. Keyboard
modes and user configuration can change bindings. A native screenshot for this
page is still pending; the retained upstream reference image is not presented
as a screenshot of Workdeck.

## Review a commit

```sh
workdeck show
workdeck show HEAD~1
workdeck show HEAD~1 -- src README.md
```

The first command reviews the latest commit. A target is a Git ref or, in a
Jujutsu or Sapling workspace, a provider-native revision expression. Paths after
`--` filter the review. Full provider and edge-case parity remains a release gate.

## Keep the review fresh

```sh
workdeck diff --watch
```

Watch mode reloads supported file- and repository-backed input. Leave the review
open while editing, and use `q` when finished. Live reload does not imply that
every upstream selection or watcher transition is already parity-qualified.

## Bring in an agent

Keep the reviewer open. In another terminal, ask your coding agent to run
`workdeck skill path` and follow the returned review skill. Workdeck's review
sessions do not take over agent process or PTY ownership from Herder. The full
agent-review guide is still being ported.

Adapted from Hunk's MIT-licensed quick-start documentation, Copyright Modem Labs
Inc. Its complete source interval remains tracked as unmapped until the remaining
content, screenshot and behavioral evidence are accounted for.
