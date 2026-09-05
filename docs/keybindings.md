# Keybindings

Every keyboard shortcut is a named command. The user-only `[keybindings]` table maps command IDs to the keys you want them on:

```toml
[keybindings]
"workdeck.app.quit" = "ctrl+x"               # one chord
"workdeck.review.nextHunk" = ["]", "ctrl+n"] # several chords for one command
"workdeck.review.focusFilter" = "f"          # takes f away from page-down
"workdeck.view.toggleMenuBar" = false        # unbind it entirely
"myext.toggle" = "ctrl+g"                    # extension commands too
```

Every ID begins with its owner. Workdeck’s built-ins use `workdeck.`; extension commands use the extension ID. `workdeck` is reserved, so an extension cannot shadow a built-in command added now or later.

The resolution rules are:

- A user binding replaces all defaults for that command; it is not additive.
- A chord explicitly claimed by one command is removed from any other command that held it only by default. That old owner keeps its remaining keys.
- `false` or `[]` unbinds a command without removing its programmatic command ID.
- When two user entries claim the same chord, the first entry in the file wins. The session reports the conflict, unknown IDs, and unusable chords while applying the rest of the table.
- A compatibility alias configures its canonical command. The first alias or canonical entry in the file wins if both appear.

Chords join `ctrl`, `alt`/`option`, `cmd`/`meta`, and `shift` to a base with `+`. A base is a character such as `y` or `[`, an uppercase letter such as `G`, or a named key such as `tab`, `pageup`, `left`, or `f2`. `shift` applies to letters and named keys. For a shifted symbol or digit, bind the produced character (`!`, not `shift+1`) because that is what terminals report. `ctrl+<letter>` also matches an unnamed bare control byte; named Tab and Enter remain distinct.

Inline saved notes expose Edit, Reply, and, for reply-free user notes, Delete. `E` edits the first editable user note in the selected hunk; `R` replies to its first visible stored note. Replies inherit the code anchor and can be nested without a product depth limit. Static sidecar annotations are not reply targets, and a parent cannot be deleted until its replies are removed.

## Built-in commands

| Command id | Does | Default keys |
| --- | --- | --- |
| `workdeck.app.openAgentSkill` | Show agent skill | _(none)_ |
| `workdeck.app.quit` | Quit | `q` |
| `workdeck.app.refresh` | Refresh the review | `r` |
| `workdeck.app.toggleFocusArea` | Switch focus between files and filter | `tab` |
| `workdeck.app.toggleHelp` | Toggle help | `?` |
| `workdeck.review.alignCurrentLineBottom` | Align current line to viewport bottom | _(none)_ |
| `workdeck.review.alignCurrentLineCenter` | Center current line in viewport | _(none)_ |
| `workdeck.review.alignCurrentLineTop` | Align current line to viewport top | _(none)_ |
| `workdeck.review.editActiveNote` | Edit the active review note | `E` |
| `workdeck.review.editSelectedFile` | Open the selected file in your editor | `e` |
| `workdeck.review.focusFilter` | Focus the file filter | `/` |
| `workdeck.review.halfPageDown` | Scroll down half a page | `d`, `ctrl+d` |
| `workdeck.review.halfPageUp` | Scroll up half a page | `u`, `ctrl+u` |
| `workdeck.review.jumpToBottom` | Jump to end | `G`, `end` |
| `workdeck.review.jumpToTop` | Jump to start | `g`, `home` |
| `workdeck.review.nextAnnotatedFile` | Next annotated file | _(none)_ |
| `workdeck.review.nextAnnotatedHunk` | Next annotated hunk | `}` |
| `workdeck.review.nextFile` | Next file | `.` |
| `workdeck.review.nextHunk` | Next hunk | `]` |
| `workdeck.review.pageDown` | Scroll down one page | `pagedown`, `space`, `f` |
| `workdeck.review.pageUp` | Scroll up one page | `pageup`, `b`, `shift+space` |
| `workdeck.review.previousAnnotatedFile` | Previous annotated file | _(none)_ |
| `workdeck.review.previousAnnotatedHunk` | Previous annotated hunk | `{` |
| `workdeck.review.previousFile` | Previous file | `,` |
| `workdeck.review.previousHunk` | Previous hunk | `[` |
| `workdeck.review.replyToActiveNote` | Reply to the active review note | `R` |
| `workdeck.review.scrollCodeLeft` | Scroll code left; Shift scrolls faster | `left`, `shift+left` |
| `workdeck.review.scrollCodeRight` | Scroll code right; Shift scrolls faster | `right`, `shift+right` |
| `workdeck.review.startNote` | Add a review note | `c` |
| `workdeck.review.stepDown` | Scroll down one row | `down`, `j` |
| `workdeck.review.stepUp` | Scroll up one row | `up`, `k` |
| `workdeck.review.toggleHunkGap` | Expand or collapse selected context | `z` |
| `workdeck.view.applyFilePresentationToAllMatching` | Apply current file presentation to all matches | _(none)_ |
| `workdeck.view.cursorLineNumber` | Mark the current line number | _(none)_ |
| `workdeck.view.cursorLineOff` | Hide the current-line marker | _(none)_ |
| `workdeck.view.cursorLineRow` | Highlight the current row | _(none)_ |
| `workdeck.view.layoutAuto` | Auto layout | `0` |
| `workdeck.view.layoutSplit` | Split layout | `1` |
| `workdeck.view.layoutStack` | Stack layout | `2` |
| `workdeck.view.openThemeSelector` | Choose theme | `t` |
| `workdeck.view.toggleAgentNotes` | Toggle agent notes | `a` |
| `workdeck.view.toggleCopyDecorations` | Toggle copy decorations | _(none)_ |
| `workdeck.view.toggleFilesPane` | Toggle files pane | `s` |
| `workdeck.view.toggleHunkHeaders` | Toggle hunk headers | `m` |
| `workdeck.view.toggleLineNumbers` | Toggle line numbers | `l` |
| `workdeck.view.toggleLineWrap` | Toggle line wrapping | `w` |
| `workdeck.view.toggleMenuBar` | Toggle menu bar | `M` |

The files-pane command follows the named `workdeck:files` role. If an extension replaces that role, the command and View → Files pane toggle the resolved replacement on any terminal edge without changing unrelated panes. Remapping or unbinding `workdeck.view.toggleFilesPane` changes the role-aware action, not an extension pane’s own commands. `workdeck.view.toggleSidebar` remains a compatibility alias; use the files-pane name in new config and extension code.

Commands marked _(none)_ ship without a chord. They remain callable by command ID and may be assigned through `[keybindings]`; some also appear in menus, while semantic actions such as current-line alignment need no menu entry.

Menus and the `?` help dialog render keys from the resolved command table. A remap therefore changes what they advertise. Unbinding a menu command retains the item without a key label.

Extension commands use `<extensionId>.<commandId>` and resolve by the same rules. An activated extension keyboard mode is a routing layer, not another command table: it may consume a key, pass it to resolved bindings, or consume it and exit. Multi-key grammar and counts are extension-owned, but resolved actions invoke these same public `workdeck.*` commands.

Routing precedence is host prompts and dialogs, menus and overlays, focused text inputs (including host-rendered extension pane inputs), interactive file-view mode, session extension keyboard mode, the command table, then the focused review widget. Widget-owned keys such as Esc, Enter, and Ctrl+S while composing a note are structural and are not remappable. Escape is reserved for exiting active extension modes so an extension cannot trap the keyboard.

`[keybindings]` is read from `~/.config/workdeck/config.toml` only, never from repository configuration. A checkout cannot rearrange the reviewer’s keyboard.
