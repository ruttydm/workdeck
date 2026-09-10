# Native extension events and pane actions

Workdeck extension API v1 keeps terminal rendering and input ownership in the host while allowing stateful review tools such as the bundled review-triage example.

## Public commands

Command invocations, keyboard-mode lifecycle/key requests, and dialog continuations carry a frozen `commands.enabled` projection of the live public Workdeck command table. It contains canonical IDs and public compatibility aliases. `commands.is_enabled(id)` is a non-throwing probe, while `commands.execute(id, count)` rejects an invalid ID/count and otherwise returns a declarative execution action only when that ID is currently enabled. The host resolves and validates the action again before dispatch, and command epochs discard results returned by a retired review handler.

Each command invocation also carries a frozen provider-neutral `selection`: the selected extension file, a hunk index clamped to that file's actual hunks, and an optional one-based source line. Missing, filtered-out, binary, and stale selections resolve to explicit null fields. Rust ownership replaces JavaScript object freezing, so retained handlers cannot mutate the review through the snapshot.

Review navigation is guarded again when a declarative action reaches the host. Targets resolve from the files visible at that moment, hunk indexes clamp to the live file, invalid addresses are refused, and an absent source line produces an attributed warning. Navigation returned by a command from a retired review generation is discarded with the reload warning; a line hidden inside a collapsed region may still land quietly on its containing hunk.

Line-granular review movement is derived from measured Ratatui row geometry, not reparsed patch order. Stable row anchors preserve split-side ordering, context-row aliases, expanded-gap identity, and reload recovery; an indexed, identity-stable cursor list keeps keypress stepping constant-time and clamps the marker to fully visible viewport rows.

An opted-in pane's current-line paint is built only from the accepted split-row plan and exact stable cursor. The native snapshot's `render(side, width)` helper returns a declarative `ViewNode::CurrentLine`; the host accepts it only from a pane that received a live current-line context and only up to that pane's allocated width. Ratatui resolves the node from the same highlighted split-row plan at paint time, adapting either half to a clipped, no-wrap row while preserving source address, movement paint, spans, line-number policy, horizontal offset, and theme. File-view components cannot smuggle this pane-scoped node into another renderer. Pending plans and mismatched cursor identities expose no stale painter.

Pane registration now carries Hunk's replacement, current-line, and availability opt-ins across native JSON-RPC. The Ratatui session registry prepends the built-in Files pane, applies first-registration and first-replacement ownership, preserves known open choices across extension reloads, and resolves replacement chains by stable qualified key. Availability is probed through a controller-supplied native transport evaluator before the pure four-edge geometry pass; pending current-line panes are retained only by monotonic registration identity, never by a stale same-key registration.

The committed pane controller owns that evaluator and all state transitions that must not occur during paint. An `available = true` registration receives `workdeck/pane/available` with the public files, selection, placement, and opted-in current-line address; the host enforces the normal request deadline and accepts only a boolean response. Callback and render failures quarantine the exact registration, warn once, cancel its drag, and restore the built-in Files role when a replacement fails. Size overrides retain their width/height axis so a same-key reload cannot reinterpret columns as rows.

The mounted review shell performs that probe against the currently filtered file projection before each changed availability signature reaches four-edge planning. False results hide the pane without closing its logical state, so clearing a filter can restore it; failures quarantine only the throwing registration. The named Files role remains one toggle across left, right, top, or bottom replacements, and a failed replacement's injected built-in fallback can be closed and reopened by the same command.

Explicit native `OpenPane` and toggle-open actions for left/right panes reveal the
side area instead of leaving the newly opened pane suppressed by automatic
sidebar visibility. Closing a pane does not hide the area or close its siblings.
The four-edge planner still enforces available terminal space. This matches the
pinned controller's `revealIfSide` transition; the host regression
`extension_side_pane_open_reveals_area_but_close_does_not_hide_it` covers both side
placements and both opening actions. The native launch capture subsequently
passed all six triage frames, including saved rationale and a second decision,
plus both raw/rendered palette and dependency file-view pairs (10 frames across
three sessions). This is a native integration check, not dual-baseline parity.

Extension commands share one resolved session keymap with built-ins. The native command table probes built-in matchers first and then prior extension registrations in load order, removes only each conflicting chord, and derives dispatch plus menu labels from the accepted set. Commands with no remaining chord stay available in the Extensions menu. A user remap replaces the extension's declared defaults, and moving a built-in releases its former chord for an extension to claim.

Every `workdeck/pane/render` request carries the filtered files, selected file and hunk, optional current line, paint theme, exact allocated cells, and `ExtensionResolvedKeybindings`. Its `get_keys` and `matches` helpers use the shared chord grammar; bindings removed by live conflict resolution are absent. A declarative `ViewNode::List` with a selected item follows that item inside its Ratatui viewport, providing the native equivalent of Hunk's documented `scrollChildIntoView` recipe without exposing renderer objects across JSON-RPC.

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

Keyboard ownership has a monotonic activation identity independent of the extension and mode IDs.
Normal extension commands may enter a mode, while actions returned from `onEnter` or `onExit` cannot
change ownership. Actions returned from a key request are scoped to the activation that received the
key: if that response installs a replacement, a later exit action or exit result from the predecessor
cannot retire it. Failed entry runs teardown once, host exit clears ownership before invoking exit,
and dropping or reloading the review tears the mode down exactly once. The native subprocess protocol
cannot retain JavaScript control closures; activation-scoped response batches provide the equivalent
old-context isolation.

## Clickable pane rows

An extension wraps a declarative `ViewNode` in `ViewNode::Action { id, child }`. Ratatui renders the child, records its visible cell rectangle, and sends `workdeck/pane/action` with the pane ID, action ID, current review snapshot, semantic saved-note snapshot, working directory, and open panes. The extension answers with ordinary validated host actions. Action IDs are local opaque values, limited to 1,024 bytes; they cannot carry executable callbacks across the process boundary.

`RefreshPane` invalidates only the named extension pane. This replaces component-framework rerenders with explicit, bounded host cache invalidation.

## Focused pane inputs

`ViewNode::Input { id, value, placeholder, focused }` is the native, host-rendered equivalent of Hunk's controlled one-line pane editor. IDs are unique within one pane tree and limited to 1,024 bytes; values are limited to 64 KiB; IDs, values, and placeholders cannot contain line endings; and one tree may declare at most one focused input. Across simultaneously visible panes, the first focused input in deterministic pane-layout order owns typing.

Ratatui draws the value (or its placeholder), measures its cell width, and owns the terminal cursor. Character insertion, Unicode-aware Left/Right/Home/End movement, Backspace, and Delete are delivered as a complete controlled value through `workdeck/pane/input`. The request carries the local pane/input IDs, current review and saved-note snapshots, working directory, and open panes. The extension commits the value in its own state and returns ordinary validated host actions; Workdeck invalidates the pane render immediately. Protocol validation and the normal two-second request deadline contain malformed or stalled handlers.

A focused pane input runs after prompts, dialogs, menus, help, theme selection, and the file filter, but before interactive file-view modes, session keyboard modes, extension commands, built-in commands, and review scrolling. This preserves Hunk's editor-first routing: text such as `j?` cannot both edit the pane and invoke navigation, extension commands, or help. Inputs are pane-only and are rejected from fixed-height file-view components, whose keyboard contract remains deliberately non-focusable.

## Dialogs

Input dialogs accept an optional initial value. Select, input, confirmation, and host-mediated workspace-write prompts enter one global FIFO, so requests from different extensions cannot replace or jump ahead of the visible question. Promoting a request resets its option cursor or input value; select movement wraps at both ends, and a stale answer ID cannot settle the request behind it.

Workdeck normalizes and terminal-sanitizes extension text, preserves edge spaces in input initial values, limits confirmation prose to six authored lines, and supplies `ok`/`cancel` labels when an extension leaves them blank. Native extension prompts are attributed to their extension; host-compiled UI may explicitly omit that row. The host renders every modal and returns a fresh review context to the owning subprocess. Confirmation accepts Enter or `y` and cancels with Escape or `n`.

A soft reload cancels the visible request and the complete queue before replacement lifecycle events are delivered, but leaves the controller open for the new review. App teardown closes and drains the queue, and runtime replacement cannot leave a request owned by a retired subprocess on screen.

## Events

Extensions declare closed-set Hunk lifecycle names in `EventSubscription` and open nonblank
extension-bus names in `CustomEventSubscription`. The host queues `workdeck/event` in extension
load order with immutable review snapshots and a committed event-context snapshot, then returns to
Ratatui without waiting for a subprocess. Each extension has one chronological request queue shared
by events and commands; slow handlers cannot stall input, rendering, or other extensions, while
events and commands observed by one extension retain their original order. That context carries the
review working directory and the owning extension's currently open local pane IDs; `sidebars` is the
exact state alias for `panes`. Native callbacks request notifications, pane changes, navigation,
dialogs, and further events by returning their corresponding declarative host actions.

Extensions can also emit independent newline-delimited `workdeck/notify` JSON-RPC
notifications. The stdout reader delivers those to the shared notification hub
before routing subsequent responses or EOF to the request waiter. Thus an emitted
notification is not discarded when the transform subsequently returns a JSON-RPC
error, returns an invalid changeset, or exits abruptly. The host then warns and
retains the previous changeset. `examples/tests/transform_notifications.rs`
executes all three cases against a compiled child, checking notification order,
host-assigned increasing IDs, severity, and complete changeset preservation. This
is native macOS evidence, not a frozen cross-platform or dual-baseline oracle.
After adding the failure probes, `cargo test -p workdeck-examples --all-targets
-- --quiet` passed all 42 library unit tests and 230 integration tests (zero
ignored); `cargo clippy -p workdeck-examples --all-targets -- -D warnings` also
passed on the same host. No source-ledger disposition was advanced by these
supplemental failure tests.

The provider is installed only after the review app has committed, before startup events are published. Runtime replacement installs the successor before retiring the predecessor, and cleanup is identity checked so stale teardown cannot detach the newer provider. Retirement atomically changes the registry from ready to closing before any shutdown work, making retained pane, navigation, dialog, and event capabilities inert immediately. Every process subscribed to `shutdown` receives at most one best-effort retirement notification, all retiring processes share one 250 ms deadline, and uncooperative children are terminated. Dropping the review app removes the active provider.

The complete pinned lifecycle set is:

- `startup`
- `changeset_loaded`
- `command_executed`
- `selection_changed`
- `file_viewed`
- `hunk_viewed`
- `filter_changed`
- `theme_changed`
- `layout_changed`
- `watch_reload_pending`
- `note_created`
- `note_edited`
- `note_changed`
- `session_reload`
- `shutdown` (delivered through the retirement notification after authority is revoked)

Ratatui commits selection attention through an explicit 150 ms trailing state machine. Rapid
navigation collapses to the last selection; a replaced registry or unmounted review invalidates
retired work even if its old deadline is forced to fire. `file_viewed` follows file-projection
identity, so a soft reload reports a replacement file object even when its stable ID is unchanged;
`hunk_viewed` follows registry, file ID, and hunk index, so that same reload does not invent another
hunk transition. Initial note, filter, layout, and theme projections seed a registry silently.
Saved-note changes are diffed only within one review generation, while draft edits and committed
user-note actions publish their complete public note payload immediately. Reload payloads identify
`watch`, `daemon`, or `manual` provenance, and all command entry paths publish the canonical built-in
or namespaced native command ID after synchronous dispatch.

An extension may emit a custom event through `EmitEvent`. As in pinned Hunk, any non-blank string
is accepted; namespacing remains recommended but spaces, Unicode, lifecycle-shaped names, and the
product prefix are not silently rejected. Events emitted while a native factory constructs its
handshake are returned as `PendingCustomEvent` declarations. Ratatui removes those provisional
declarations, waits until every extension subscription is registered, and replays them in original
extension/declaration order. The host appends broadcasts to each current subscriber's chronological
queue, validates every returned action, and carries causal depth across asynchronous responses so
recursive event chains still stop at depth 16. Timed-out request IDs are revoked and late replies are
discarded before a later request is decoded. A crashed, timed-out, unsubscribed, or malformed
extension cannot inject an unchecked action or retain terminal ownership.

Hunk freezes JavaScript envelopes, file arrays, files, metadata, stats, agent annotations, and hunk summaries before invoking in-process handlers. Native Workdeck crosses an NDJSON subprocess boundary instead: `ReviewEvent`, `ReviewSnapshot`, `ExtensionDiffFile`, and nested JSON payloads are owned Rust values serialized separately for each process. A child may mutate its local deserialized copy, but it cannot reach another handler's value or live review state. File projections always derive change type and hunk summaries from the current parsed `DiffFile`; binary or skipped files carry an empty hunk vector. `Arc`-backed preparation snapshots preserve reuse inside the host without exposing reference identity as public API.

The review-triage example demonstrates reconciliation: decisions, viewed marks, note counts, and current selection are session-local and retained only while their file/hunk address still exists after a load or reload.
