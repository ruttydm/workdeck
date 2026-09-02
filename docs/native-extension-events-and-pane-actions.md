# Native extension events and pane actions

Workdeck extension API v1 keeps terminal rendering and input ownership in the host while allowing stateful review tools such as the bundled review-triage example.

## Public commands

Command invocations, keyboard-mode lifecycle/key requests, and dialog continuations carry a frozen `commands.enabled` projection of the live public Workdeck command table. It contains canonical IDs and public compatibility aliases. `commands.is_enabled(id)` is a non-throwing probe, while `commands.execute(id, count)` rejects an invalid ID/count and otherwise returns a declarative execution action only when that ID is currently enabled. The host resolves and validates the action again before dispatch, and command epochs discard results returned by a retired review handler.

Each command invocation also carries a frozen provider-neutral `selection`: the selected extension file, a hunk index clamped to that file's actual hunks, and an optional one-based source line. Missing, filtered-out, binary, and stale selections resolve to explicit null fields. Rust ownership replaces JavaScript object freezing, so retained handlers cannot mutate the review through the snapshot.

## Clickable pane rows

An extension wraps a declarative `ViewNode` in `ViewNode::Action { id, child }`. Ratatui renders the child, records its visible cell rectangle, and sends `workdeck/pane/action` with the pane ID, action ID, current review snapshot, semantic saved-note snapshot, working directory, and open panes. The extension answers with ordinary validated host actions. Action IDs are local opaque values, limited to 1,024 bytes; they cannot carry executable callbacks across the process boundary.

`RefreshPane` invalidates only the named extension pane. This replaces component-framework rerenders with explicit, bounded host cache invalidation.

## Dialogs

Input dialogs accept an optional initial value. Select, input, confirmation, and host-mediated workspace-write prompts enter one global FIFO, so requests from different extensions cannot replace or jump ahead of the visible question. Promoting a request resets its option cursor or input value; select movement wraps at both ends, and a stale answer ID cannot settle the request behind it.

Workdeck normalizes and terminal-sanitizes extension text, preserves edge spaces in input initial values, limits confirmation prose to six authored lines, and supplies `ok`/`cancel` labels when an extension leaves them blank. Native extension prompts are attributed to their extension; host-compiled UI may explicitly omit that row. The host renders every modal and returns a fresh review context to the owning subprocess. Confirmation accepts Enter or `y` and cancels with Escape or `n`.

A soft reload cancels the visible request and the complete queue before replacement lifecycle events are delivered, but leaves the controller open for the new review. App teardown closes and drains the queue, and runtime replacement cannot leave a request owned by a retired subprocess on screen.

## Events

Extensions declare every subscribed event in one `EventSubscription` registration. The host sends `workdeck/event` in extension load order with immutable review snapshots and a committed event-context snapshot. That context carries the review working directory and the owning extension's currently open local pane IDs; `sidebars` is the exact state alias for `panes`. Native callbacks request notifications, pane changes, navigation, dialogs, and further events by returning their corresponding declarative host actions.

The provider is installed only after the review app has committed, before startup events are published. Runtime replacement installs the successor before retiring the predecessor, and cleanup is identity checked so stale teardown cannot detach the newer provider. Dropping the review app removes the active provider.

Lifecycle events currently include:

- `changeset_loaded`
- `session_reload`
- `selection_changed`
- `hunk_viewed`
- `note_created`
- `filter_changed`
- `watch_reload_pending`

An extension may emit a custom event through `EmitEvent`. Custom names require a nonempty namespace and event separated by `:`; the `workdeck:` namespace is reserved. The host broadcasts synchronously to current subscribers, validates every returned action, and stops recursive event chains at depth 16. A crashed, timed-out, unsubscribed, or malformed extension cannot inject an unchecked action or retain terminal ownership.

The review-triage example demonstrates reconciliation: decisions, viewed marks, note counts, and current selection are session-local and retained only while their file/hunk address still exists after a load or reload.
