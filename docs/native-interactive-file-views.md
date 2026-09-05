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

File-presentation controls are host actions resolved when they are applied, so retained command
results observe the current selection, draft-note mask, and extension registry. `select-file-view`
selects a named view or raw diff when `id` is absent; `toggle-file-view` preserves the concise
toggle path. Bare view ids belong to the calling extension. A qualified `extension:view` id may
target another live registration, matching Hunk's unified file-view registry. Hard-remount cleanup
revokes the predecessor runtime before a successor can accept its actions.

Keys not claimed by host modals are delivered synchronously through
`workdeck/file-view-mode/key`. The result is `handled`, `pass`, or `exit` plus validated host
actions. `pass` preserves Workdeck navigation, help, command, and quit behavior. A stateful view
returns `refresh-file-view` after a state transition; a `fileId` scopes invalidation to one file.
The next Ratatui frame schedules a fresh deterministic layout. Preparation runs outside paint with
a 1.5-second source-and-RPC budget and four global worker slots. Exact file, view, registration,
width, and refresh-epoch identities drive a 64-entry LRU. Width and registration changes suppress
stale geometry synchronously; an epoch refresh keeps the compatible prior tree visible until the
replacement pass settles. Late results from a superseded pass are ignored.

The subprocess receives immutable old/new snapshots so its response cannot race a reload. Once a
layout is returned, Workdeck invokes provider-backed source reads only for sides actually named by
row bindings, then validates every inclusive one-based range before Ratatui sees the tree. Source
reads share the layout deadline; a blocked read releases its preparation slot while its underlying
provider operation may finish harmlessly later. Match and layout failures fall back to raw diff and
are deduplicated per concrete registration and deterministic failure category.

Lifecycle responses may contain ordered `actions` and an optional contained `failure`. Workdeck
applies the actions before reporting the failure and retires only the activation that failed. This
lets an outgoing callback hand off to a replacement mode without the old callback later tearing
that replacement down. Activation ids apply the same ownership rule to `exit` key results. Native
lifecycle handoffs are limited to 32 nested transitions to contain accidental recursion.

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
