+++
title = "Keyboard and mouse"
description = "Navigate, scroll, filter and change the native review view."
template = "docs.html"
+++

Press `?` at any time for Workdeck's in-app shortcut reference. Menus and primary review actions are also mouse-selectable.

## Navigate the review

| Keys                      | Action                                                |
| ------------------------- | ----------------------------------------------------- |
| `↑` / `↓`, `k` / `j`      | Scroll one row                                        |
| `Space` / `f`, `b`        | Page down / up                                        |
| `Shift+Space`             | Page up                                               |
| `d` / `u`                 | Half page down / up                                   |
| `[` / `]`                 | Previous / next hunk                                  |
| `,` / `.`                 | Previous / next file                                  |
| `{` / `}`                 | Previous / next annotated hunk                        |
| `Home` / `End`, `g` / `G` | Start / end of review                                 |
| `←` / `→`                 | Scroll unwrapped code; hold Shift for faster movement |

Workdeck navigation stays review-wide: hunk and file shortcuts move through the same multi-file stream shown in the main pane.

`↑` / `↓` and `k` / `j` move a highlighted current line, and the view scrolls only far enough to keep it visible. Paging or scrolling past it moves it to the nearest line still on screen, and `c` anchors a note on it. Pick the marker from the View menu, or set `cursor_line`: `number` marks only the line number, and `off` turns the marker off and lets `↑` / `↓` and `k` / `j` scroll the view one row at a time instead.

## Change the view

| Key             | Action                                           |
| --------------- | ------------------------------------------------ |
| `0` / `1` / `2` | Auto / split / stack layout                      |
| `s`             | Toggle files pane                                |
| `t`             | Choose a theme                                   |
| `l`             | Toggle line numbers                              |
| `w`             | Toggle line wrapping                             |
| `m`             | Toggle hunk metadata                             |
| `M`             | Toggle menu bar                                  |
| `z`             | Toggle unchanged context for the selected hunk   |
| `a`             | Toggle agent notes                               |
| `e`             | Open the selected file in `$EDITOR`              |
| `/`             | Focus file filter                                |
| `Tab`           | Move focus between the file list and file filter |
| `r`             | Reload a reloadable input                        |
| `F10`           | Open menus                                       |
| `q`             | Quit                                             |

Workdeck may offer to save view changes on quit. Check the destination displayed
by the prompt; the default personal configuration is
`~/.config/workdeck/config.toml`. Initial viewing does not require saving settings.

## Add a human note

Press `c` on the selected hunk or use a visible add-note affordance with the mouse. While editing, app shortcuts are suspended so normal text entry works. Save with the note editor's displayed action or cancel with Escape.

## Mouse behavior

- Click a sidebar file to jump to it in the review stream.
- Click menus and dialog actions instead of their key equivalents.
- Use the wheel or scrollbar to move through the review; hold Shift while scrolling to move horizontally through unwrapped code.
- Select diff text for copy where the terminal supports it.

## Remap the defaults

Every shortcut above is a named command you can move to different keys with a `[keybindings]` table in your user config. See [Keybindings](/docs/configure/keybindings/).

Terminal mouse protocols vary; see the repository's terminal compatibility documentation (the website guide is still being migrated) if clicks or selection do not behave as expected.

Adapted from the pinned Hunk MIT documentation, Copyright Modem Labs Inc.
The linked configuration and compatibility guides are still being migrated;
this source interval remains unmapped. Native command tests and capture checks
are not a claim of complete keyboard, mouse or terminal parity.
