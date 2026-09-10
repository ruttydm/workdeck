+++
title = "Layout and display"
description = "Configure review layout, code rows, chrome and note visibility."
template = "docs.html"
+++

Workdeck uses the same normalized review model in every layout.

## Pick a layout policy

```bash
workdeck diff --mode auto
workdeck diff --mode split
workdeck diff --mode stack
```

- `auto` chooses split on wide terminals and stack on narrow ones.
- `split` keeps before and after columns side by side.
- `stack` shows changed rows in a single-width flow.

Explicit split and stack choices override responsive behavior. Press `0`, `1`, or `2` to switch while reviewing.

## Tune code rows

```bash
workdeck diff --no-line-numbers --wrap --no-hunk-headers --tab-width 2 --file-gap 3 --hunk-gap 1
```

Paired flags let scripts express either state: `--line-numbers` / `--no-line-numbers`, `--wrap` / `--no-wrap`, and `--hunk-headers` / `--no-hunk-headers`. Tab width accepts an integer from 1 through 16. `--file-gap` is separator height including the `─` rule (0 hides it); `--hunk-gap` is blank rows before later hunks. Both accept 0 through 8.

## Tune review chrome

TOML settings cover persistent display details:

```toml
mode = "auto"
line_numbers = true
wrap_lines = false
hunk_headers = true
file_gap = 1
hunk_gap = 0
menu_bar = true
sidebar = "auto"
agent_notes = false
copy_decorations = false
transparent_background = false
cursor_line = "row"
```

`transparent_background` lets the terminal paint Workdeck surfaces; turn it off when exact theme surfaces matter more than matching terminal transparency.

`cursor_line` chooses how the line you are on is marked: `row` highlights the whole row, `number` marks only its line number, and `off` removes the marker and returns `k` / `j` to scrolling the view one row at a time. Switch it mid-review from the View menu, or set `--cursor-line <style>` for a single run.

`file_gap` is the number of rows between files, including the `─` rule. `1` is the current look; `0` hides the rule; larger values add blank rows above it. `hunk_gap` inserts blank rows before each hunk after the first in a file.

Use `~/.config/workdeck/config.toml` for personal settings. Read-only viewing
need not create repository state. Adapted from Hunk's pinned MIT documentation,
Copyright Modem Labs Inc. The native flags and configuration model support the
options above; exhaustive layout, theme and terminal parity remains unverified.

