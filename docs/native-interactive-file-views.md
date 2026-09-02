# Native interactive file views

Workdeck's native extension API attaches an optional keyboard mode to a declarative file
presentation. The extension remains a newline-delimited JSON-RPC subprocess; Ratatui owns every
terminal cell, modal, pointer target, and filesystem prompt.

An extension declares `interactiveMode = true` on a `file-view` registration. A command may return
`enter-file-view-mode`, which asks the host to verify the registration and current file match,
select that presentation for the file, exit any other extension mode, and call
`workdeck/file-view-mode/enter`. The selected file, presentation, registration, and review
generation form one lifecycle identity. Selection changes, presentation changes, reloads, Escape,
extension failures, and shutdown exit the mode through `workdeck/file-view-mode/exit`.

Keys not claimed by host modals are delivered synchronously through
`workdeck/file-view-mode/key`. The result is `handled`, `pass`, or `exit` plus validated host
actions. `pass` preserves Workdeck navigation, help, command, and quit behavior. A stateful view
returns `refresh-file-view` after a state transition; a `fileId` scopes invalidation to one file.
The next Ratatui frame asks the subprocess for a fresh deterministic layout.

Workspace mutation stays host-owned. A mode with the `workspace-write` capability may return
`request-workspace-write`, but it cannot touch the filesystem through the protocol. Workdeck shows
an `ext <extension-id>` consent prompt, then accepts a replacement only when all of these facts are
still true:

- the review is an unstaged working-tree changeset;
- the file has an attested working-tree source snapshot;
- its path is repository-relative and contains only normal components;
- neither the target nor its canonical parent escapes through a symlink;
- the current text still equals the source snapshot shown by the review.

The result is returned through `workdeck/workspace/write-complete` as `written`, `cancelled`, or
`failed`. Cancellation is not an error. A successful write reloads the review and exits the mode;
failed and cancelled writes retain the edit buffer. Message and action limits, request deadlines,
process crash isolation, capability checks, and terminal sanitization remain enforced by the native
host.

The complete executable reference is
[`examples/extensions/inline-edit/`](../examples/extensions/inline-edit/). Its oracle and Rust tests
cover line endings, Unicode graphemes, terminal-cell truncation, key pass-through, buffer lifecycle,
scoped refresh, source provenance, hunk geometry, consent, stale-write rejection, subprocess
round-trips, and Ratatui frames.
