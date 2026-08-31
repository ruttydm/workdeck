# Workdeck Navigation Model

## Global rail

The 44px rail owns Updates, Workspaces, Git, CI, Search, and bottom-anchored Artifacts. Git is one workspace: its compact titlebar tabs switch directly between Commits and Pull requests without adding another rail destination or content toolbar.

Keyboard destinations are `⌘1` Updates, `⌘2` Workspaces, `⌘3` Git commits, `⌘4` Git pull requests, `⌘5` CI, `⌘6` Search, and `⌘7` Artifacts; `⌘K` opens the command palette. Pointer activation, Enter/Space activation, focus styling, tooltip copy, selected state, and accessible names stay equivalent.

## Workspace header and task context

The single 52px workspace header contains the current context, refresh, command search, and—only inside Git—the Commits/Pull requests task switcher. Changes is not a permanent destination. Selecting a PR, commit, range, or reference updates the center diff in place while retaining the activity list on the left and changed-files tree on the right.

The selected commit, changed file, diff layout, scroll position, range endpoint, PR, run/job, artifact, and search context are keyed by stable IDs rather than row numbers. The first available PR or WIP row is selected automatically so a task never opens on a needless blank state.

## Navigator

The context navigator exists only where it is the primary scope switcher: Workspaces shows a compact project index. Selecting a project filters the workbench’s complete Project → Repository → Checkout → Worktree hierarchy, so the sidebar never duplicates every nested row. Updates, Git, Search, Pull requests, CI, Artifacts, and Changes already own their task-specific list, graph, search result, or changed-file tree.

Workspaces uses true indentation and disclosure. Unavailable/prunable history is retained and routed to an explanatory detail rather than a dead end.

## Activity flow

```text
Unread commit branch / pull request
    → Git or Pull requests
        → commit range / PR changes
            → Diff / Split / Source / Markdown / Calls / Structure / AST
                → automatically read at the exact source revision
```

Read state belongs to the commit branch or PR activity revision. A later source revision makes it unread again. Workdeck never asks the user to create a deck, capture a checkpoint, or complete every file.

## Back, Escape, and dismissal

Escape closes the command palette or native dialog, closes an artifact preview, then restores focus to the shell. Dismissed modal focus returns to the invoking control. Refresh never changes destination or selection unless the selected object truly no longer exists.

## Responsive navigation

Only Workspaces exposes the optional generic inspector. Git commits and pull requests use task-owned resizable list and changed-file panes around the diff. PR, commit, CI, artifact, changed-file, and canonical-tree panes provide pointer resizing, keyboard resizing, double-click reset, and explicit collapse/reveal controls. At minimum width, the three task columns remain usable; either side pane can be collapsed without moving the selected task elsewhere.
