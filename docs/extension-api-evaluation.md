# Extension API field notes: Review triage

`examples/extensions/review-triage/` is a deliberately ordinary, user-installable
native extension built only against `workdeck-extension-api`. It provides a
session-local hunk triage board: a reviewer opens a right pane, navigates public
hunk summaries, marks the current hunk approved/investigate/blocked with an
optional rationale, and clears decisions. Its commands are ordinary extension
menu entries, while lifecycle and bus events keep the board current.

Building it validates the same central path as the pinned Hunk fixture: a
third-party extension composes a declarative pane, menu-reachable commands,
host-owned modal dialogs, selection snapshots, lifecycle subscriptions,
notifications, and a small inter-extension bus without imports from Workdeck
internals. The PTY integration test loads the compiled directory rather than a
string fixture.

## Findings

### Sidebar geometry and selection following are missing

The original Hunk public sidebar props exposed width but not pane height,
viewport bounds, scroll position, or a way to scroll an item into view. Workdeck
closes that gap with a bounded `PaneScroll` contract: native panes receive exact
width and height, stable row IDs, viewport reads, scroll-into-view, and resize
subscriptions. The built-in Files pane and
`crates/workdeck-extension-host` tests exercise the same contract. Geometry,
windowing, and outer scroll ownership remain host-controlled.

### Extensions have no safe, host-managed persistence

The triage board remains session-local unless an extension explicitly uses the
Workdeck store API. Extension configuration is repository-overridable and
untrusted for exec-adjacent decisions; using it as writable storage would be
wrong. Native extensions may request namespaced user/repository storage through
the host, which owns locations, lifecycle, privacy, atomic writes, and trust.
Review-generation and changeset identities are required when reconciling a saved
decision, so a reload cannot silently apply a stale file or hunk index.

### Command handlers cannot navigate the review stream

The pinned API gave command handlers selection snapshots, dialogs, and pane
controls but no direct `selectFile` or `selectHunk`. Workdeck exposes guarded
`commands.execute` navigation actions in the native command context as well as
pane actions. Targets resolve against the current visible projection; stale,
filtered, binary, or invalid addresses are refused with an attributed warning.
Navigation from a retired review generation is discarded.

### Dialogs are intentionally simple, but triage exposes their limits

The select/input sequence remains useful for a short status and single-line
rationale. Workdeck's dialog protocol adds labelled values, validation, bounded
multiline text, and a retained request identity while the review is live.
Reload cancellation is still mandatory: a request owned by a retired
generation cannot act on new files. Confirmation and input actions are
host-rendered and attributed to the extension.

### The Extensions menu is command-generated, not extensible layout

Commands make the extension visible in the Extensions menu and are sufficient
for this workflow. Workdeck intentionally does not let a subprocess replace the
menu bar, add arbitrary separators, or paint chrome. Declarative command state
provides titles, keybindings, enabled probes, checked state, and subcommands;
Ratatui keeps menu geometry and accessibility host-owned.

## Non-gaps confirmed by the extension

- A compiled native extension renders declarative rows in Workdeck's Ratatui
  tree and can retain its own immutable state while the pane is mounted or
  closed.
- Public hunk summaries and the pane selection/index contract are sufficient to
  render and drive a hunk-level board without accessing opaque diff metadata.
- The host's scroll contract preserves selection visibility for large reviews
  without exposing a renderer or allowing a pane to mutate outer geometry.
- Host-rendered dialogs provide attribution and modal behavior; command
  registrations provide menu and keyboard access through one mechanism.
- Lifecycle events and the namespaced bus support session-local, fire-and-forget
  coordination. The extension treats them as observers rather than persistence
  or request/response channels.

The translated native behavior is covered by the review-triage integration tests,
pane-action tests, dialog lifecycle tests, and the frozen fixture listed in
`port/hunk/oracles/extension-application.json`. Adapted from Hunk's MIT field
notes, Copyright Modem Labs Inc.
