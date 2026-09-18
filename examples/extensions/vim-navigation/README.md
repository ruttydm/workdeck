# Vim navigation extension

This compiled Rust extension adds a small Vim-normal mode across Workdeck's whole review stream. It demonstrates session keyboard modes and public semantic command execution without accessing Ratatui buffers, renderer objects, scroll boxes, or viewport coordinates. It is an explicit example rather than a bundled, automatically loaded extension.

Stage its executable and manifest, then load it explicitly:

```bash
cargo xtask extension stage-example vim-navigation
cargo run -p workdeck-cli -- --extension ./target/workdeck-extension-examples/vim-navigation diff
```

Press `F6`, or choose **Extensions → Toggle Vim navigation**, to enter or exit the mode. Workdeck displays a persistent attributed mode badge while the mode owns review-level keys. Click that badge, choose the host-owned **Exit Vim navigation** menu item, or press `Esc` to leave.

To install the staged native extension globally, copy its entire staged directory—manifest and `bin/` together—under `~/.config/workdeck/extensions/vim-navigation/`. Workdeck never executes the old TypeScript form.

## Keys

| Key | Action |
| --- | --- |
| `j` / `k` | Move the current review line down/up |
| `[` / `]` | Move to the previous/next hunk |
| `gg` / `G` | Jump to the start/end of the review |
| `zt` / `zz` / `zb` | Align the current line at the top/center/bottom |
| `Ctrl-D` / `Ctrl-U` | Move down/up by half pages |
| positive digits | Prefix the next relative motion, such as `5j` or `3]` |
| `:` | Open the host-rendered Vim command line |
| `Esc` | Exit the mode; the host consumes it before the extension |
| everything else | Pass through to Workdeck's normal routing |

Counts saturate at 10,000. Once a normal-mode sequence resolves, the extension requests one semantic action with its complete count so Workdeck applies movement atomically. A bare `0` passes through to Workdeck's layout shortcut; `0` can extend a count that began with `1`–`9`.

Pressing `:` passes the key to the registered command, which asks for a line of text inline on the status row (`ctx.prompts.line`) with a `:` prefix. That focused host input captures typed keys ahead of the still-active session mode until Enter submits, or Escape clears the buffer and a second Escape cancels.

| Command | Action |
| --- | --- |
| `:top` | Jump to the start of the review |
| `:bottom` | Jump to the end of the review |

Unsupported commands produce an attributed warning. Absolute source-line commands such as Vim's `:100` remain intentionally absent because the public semantic API does not expose source-line targeting; relative `100j` remains available.

On entry, the extension asks the host to show and position its current-line marker so the `z*` alignment commands have a target. Pending prefix and count state resets on entry and exit. Invalid continuations clear pending state and pass the current key back to Workdeck. The extension owns no terminal cells and communicates only through versioned JSON-RPC actions.
