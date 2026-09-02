# Native extension events and pane actions

Workdeck extension API v1 keeps terminal rendering and input ownership in the host while allowing stateful review tools such as the bundled review-triage example.

## Public commands

Command invocations, keyboard-mode lifecycle/key requests, and dialog continuations carry a frozen `commands.enabled` projection of the live public Workdeck command table. It contains canonical IDs and public compatibility aliases. `commands.is_enabled(id)` is a non-throwing probe, while `commands.execute(id, count)` rejects an invalid ID/count and otherwise returns a declarative execution action only when that ID is currently enabled. The host resolves and validates the action again before dispatch, and command epochs discard results returned by a retired review handler.

Each command invocation also carries a frozen provider-neutral `selection`: the selected extension file, a hunk index clamped to that file's actual hunks, and an optional one-based source line. Missing, filtered-out, binary, and stale selections resolve to explicit null fields. Rust ownership replaces JavaScript object freezing, so retained handlers cannot mutate the review through the snapshot.

Review navigation is guarded again when a declarative action reaches the host. Targets resolve from the files visible at that moment, hunk indexes clamp to the live file, invalid addresses are refused, and an absent source line produces an attributed warning. Navigation returned by a command from a retired review generation is discarded with the reload warning; a line hidden inside a collapsed region may still land quietly on its containing hunk.

Line-granular review movement is derived from measured Ratatui row geometry, not reparsed patch order. Stable row anchors preserve split-side ordering, context-row aliases, expanded-gap identity, and reload recovery; an indexed, identity-stable cursor list keeps keypress stepping constant-time and clamps the marker to fully visible viewport rows.

An opted-in pane's current-line paint is built only from the accepted split-row plan and exact stable cursor. The host adapts either half to a clipped, no-wrap full-width row while preserving source address, movement paint, spans, line-number policy, horizontal offset, and theme. Pending plans and mismatched cursor identities expose no stale painter.

Pane registration now carries Hunk's replacement, current-line, and availability opt-ins across native JSON-RPC. The Ratatui session registry prepends the built-in Files pane, applies first-registration and first-replacement ownership, preserves known open choices across extension reloads, and resolves replacement chains by stable qualified key. Availability is probed through a controller-supplied native transport evaluator before the pure four-edge geometry pass; pending current-line panes are retained only by monotonic registration identity, never by a stale same-key registration.

The committed pane controller owns that evaluator and all state transitions that must not occur during paint. An `available = true` registration receives `workdeck/pane/available` with the public files, selection, placement, and opted-in current-line address; the host enforces the normal request deadline and accepts only a boolean response. Callback and render failures quarantine the exact registration, warn once, cancel its drag, and restore the built-in Files role when a replacement fails. Size overrides retain their width/height axis so a same-key reload cannot reinterpret columns as rows.

Extension commands share one resolved session keymap with built-ins. The native command table probes built-in matchers first and then prior extension registrations in load order, removes only each conflicting chord, and derives dispatch plus menu labels from the accepted set. Commands with no remaining chord stay available in the Extensions menu. A user remap replaces the extension's declared defaults, and moving a built-in releases its former chord for an extension to claim.

Top-level extension CLI commands use a separate immutable ownership table. Exact command names are case-sensitive, the first registry claim wins, and every rejected claim becomes a source-attributed load issue. Unknown-command help lists only winners in sorted order, with Workdeck-owned branding and extension-supplied usage and summaries collapsed to one control-free terminal line. The native host stores an owned copy of registration metadata and retains each loaded manifest path for provenance.

Line-highlighter invalidation is also an explicit host action. `RefreshLineHighlights` accepts a
local highlighter ID or a qualified `extension:id`, plus an optional invocation-local file ID. A
whole-highlighter refresh and a file refresh contribute independent epochs, so neither can mask the
other. Unknown highlighters produce an attributed warning; a stale file ID racing a reload is a
silent no-op. Reload reconciliation removes retired file and registration epochs while retaining
surviving counters across native extension replacement.

An active keyboard mode retains the exact registry and registration identities that authorized it.
The Ratatui input router refuses a mode after either identity is replaced or its registry closes,
and Escape remains host-owned. Mode titles and fallback owner labels are terminal-sanitized; the
persistent badge uses `<title> — ext <extension>:<mode> — Esc exits`. Lifecycle and key failures are
contained at the subprocess boundary and reported with the same extension, mode, and callback
attribution as the pinned Hunk behavior. Native JSON-RPC deadlines replace JavaScript promise
detection; the pure Rust callback adapter also rejects deferred results explicitly for parity tests.

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
