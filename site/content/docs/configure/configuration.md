+++
title = "Configuration"
description = "Layer personal and repository TOML settings with command and CLI overrides."
template = "docs.html"
+++

Workdeck reads TOML preferences from a user file and an optional repository file:

- `~/.config/workdeck/config.toml` (or the platform/XDG config location)
- `.agents/workdeck/config.toml` at the repository root

Repository settings override user settings. Command sections then override their layer's top-level values, pager sections apply to pager-style sessions, and explicit CLI flags win last.

## Start with useful defaults

```toml
theme = "github-dark-default"
mode = "auto"
vcs = "git"
watch = false
exclude_untracked = false
line_numbers = true
tab_width = 4
file_gap = 1
hunk_gap = 0
wrap_lines = false
hunk_headers = true
menu_bar = true
sidebar = "auto"
agent_notes = false
transparent_background = false
```

Use only the keys you want to change; built-in defaults fill the rest.

## Scope a command

The following is a separate example. TOML cannot define both the scalar
`vcs = "git"` and a `[vcs]` table in the same document.

```toml
mode = "auto"

[vcs]
watch = true

[pager]
menu_bar = false
wrap_lines = true
```

Command sections are named after the input Workdeck parses, which is not always the command you type. In particular, `workdeck diff` on a repository reads `[vcs]`, not `[diff]`:

| Section        | Applies to                                                |
| -------------- | --------------------------------------------------------- |
| `[vcs]`        | `workdeck diff` working-tree and target reviews               |
| `[show]`       | `workdeck show` commit reviews                                |
| `[stash-show]` | `workdeck stash show` reviews                                 |
| `[diff]`       | two-file comparisons (`workdeck diff --files <left> <right>`) |
| `[patch]`      | `workdeck patch` reviews                                      |
| `[difftool]`   | `workdeck difftool` pair reviews                              |

`[pager]` is an overlay applied after the matching command section whenever the invocation uses pager-style behavior.

## Save interactive changes

When you change view preferences and quit, Workdeck can offer to persist them. It writes to an existing repository config when one exists; otherwise it keeps personal view choices in the user config. Set `prompt_save_view_preferences = false` to disable that prompt.

The full website config reference and extension guide are still being migrated.
The native reference schema lives in `crates/workdeck-cli/src/config.rs`.
The root-only `[extensions]` table remains separate from per-command review
settings; see the [legacy extension inventory](/extensions/) for migration limits.

Personal [keybindings](/docs/configure/keybindings/) are user-only even when a
repository supplies other settings. Initial viewing and read-only commands do
not require creating `.agents/workdeck/` or writing preferences.

Adapted from Hunk's pinned MIT configuration guide, Copyright Modem Labs Inc.
This page remains unmapped pending complete migration verification.

