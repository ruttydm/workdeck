# Native extension events and pane actions

Workdeck extension API v1 keeps terminal rendering and input ownership in the host while allowing stateful review tools such as the bundled review-triage example.

## Clickable pane rows

An extension wraps a declarative `ViewNode` in `ViewNode::Action { id, child }`. Ratatui renders the child, records its visible cell rectangle, and sends `workdeck/pane/action` with the pane ID, action ID, current review snapshot, semantic saved-note snapshot, working directory, and open panes. The extension answers with ordinary validated host actions. Action IDs are local opaque values, limited to 1,024 bytes; they cannot carry executable callbacks across the process boundary.

`RefreshPane` invalidates only the named extension pane. This replaces component-framework rerenders with explicit, bounded host cache invalidation.

## Dialogs

Input dialogs accept an optional initial value. Select, input, and confirmation dialogs are rendered by Workdeck, block other review input while open, and return a fresh review context to the owning subprocess. Confirmation accepts Enter or `y` and cancels with Escape or `n`.

## Events

Extensions declare every subscribed event in one `EventSubscription` registration. The host sends `workdeck/event` in extension load order with immutable review snapshots. Lifecycle events currently include:

- `changeset_loaded`
- `session_reload`
- `selection_changed`
- `hunk_viewed`
- `note_created`
- `filter_changed`
- `watch_reload_pending`

An extension may emit a custom event through `EmitEvent`. Custom names require a nonempty namespace and event separated by `:`; the `workdeck:` namespace is reserved. The host broadcasts synchronously to current subscribers, validates every returned action, and stops recursive event chains at depth 16. A crashed, timed-out, unsubscribed, or malformed extension cannot inject an unchecked action or retain terminal ownership.

The review-triage example demonstrates reconciliation: decisions, viewed marks, note counts, and current selection are session-local and retained only while their file/hunk address still exists after a load or reload.
