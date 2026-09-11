# Extension system architecture

This maintainer-facing map explains how Workdeck's native extension system is
assembled. The user-facing [extension guide](../site/content/docs/extend/extensions.md)
describes authoring; this document names the Rust owners, lifecycle boundaries,
and invariants that keep a review safe.

## Tiers and loading

Extensions are discovered in deterministic groups:

1. explicit `--extension` paths;
2. user paths from `~/.config/workdeck/config.toml`;
3. the managed global extension directory;
4. repository paths under `.workdeck/extensions` and its repository config,
   after a fresh trust decision.

Bundled Git, Jujutsu, Sapling, and file-navigation providers are compiled into
the distribution. They are implicitly trusted, load before user configuration,
and remain available when `--no-extensions` is supplied. User and repository
extensions are never inferred from a source-language file or a package manager.

An extension id owns the namespaces `<id>.<command>`, `<id>:<pane>`,
`<id>:<view>`, and `[extension.<id>]`. The host validates ids centrally,
rejects reserved names and duplicates, and reports each skipped candidate with
its source path. A failed manifest, spawn, handshake, or startup callback costs
only that extension; the review remains usable.

## One registry and one apply path

`workdeck-extension-host` collects session options, themes, languages, VCS
adapters, transforms, panes, file views, highlighters, commands, and event
subscriptions into one typed registry. Startup and reload both use the same
validation and apply path. A replacement is prepared and committed before the
previous process is retired; if preparation fails, the previous generation is
restored. Registry closure revokes all retained capabilities before shutdown
begins, so an old callback cannot mutate a later review.

The CLI parser resolves built-in commands first. Only an unknown top-level token
enters extension CLI discovery. The winning declaration receives raw arguments,
bounded streaming I/O, cancellation, and a one-time delegation option. Static
help/version and headless commands never start extension code unnecessarily.

## Native process boundary

`workdeck-extension-api` defines JSON-RPC 2.0 over newline-delimited stdio.
The child returns declarative `ViewNode` trees, rows, dialogs, notifications,
and semantic actions. Stderr is log-only. The host enforces message and view
depth limits, request deadlines, cancellation, duplicate registration rules,
and crash isolation. Extensions are fully trusted native programs, not
security sandboxes; repository sources remain trust-gated.

`workdeck-extension-host` owns discovery, manifest validation, process startup,
handshake, request routing, capability leases, and teardown. `workdeck-tui`
owns Ratatui cells, terminal input, geometry, mouse hit testing, and painting.
No extension receives a Crossterm writer, terminal escape sequence, Ratatui
buffer, or renderer cache.

## Four-edge panes

`workdeck-tui` resolves left, right, top, and bottom pane rectangles around the
host-owned review stream. Preferred/minimum/maximum dimensions and optional
fractions are clamped before painting; a session-local drag overrides the
automatic size without changing review coordinates. The built-in Files pane
is one qualified registration and remains the fallback when a replacement is
unavailable.

The host probes `available` panes with a bounded request using the current file
projection and current-line address. A false response hides the pane without
discarding its open choice; a failure quarantines only that registration.
Action rows carry opaque ids and a frozen review snapshot. Focused input rows
are host-rendered and route Unicode editing before modes or commands.

## File views and highlights

File-view layouts are prepared asynchronously and accepted only after schema,
row-height, source-binding, and hunk-bound checks. `workdeck-review` inserts
validated extension rows and host-owned notes into one immutable render plan;
unresolved bindings fall back to the complete raw diff rather than silently
dropping review data. A view refresh increments a scoped epoch, retaining
current rows until the replacement is ready.

Line highlighters return source-addressed ranges. `workdeck-diff` maps raw
character offsets through tabs, sanitization, grapheme clusters, wrapping, and
both review sides. Tone contrast is resolved during paint, not geometry
planning, so highlight changes cannot move the cursor or alter row counts.
Agent attention marks use the same merge and contrast pipeline. Reload
reconciliation retains a mark only when the file content identity is unchanged.

## Commands, modes, dialogs, and the status line

Built-in and extension command ids share one resolved keymap; built-ins win
conflicts and user remaps release their former chords. Keyboard modes are
activation-scoped and receive frozen key snapshots. One session mode runs at a
time; file-view modes and focused inputs temporarily outrank it. Escape, menu,
and status controls use one teardown path.

Dialogs are a host-owned FIFO. Text is terminal-sanitized, attributed to the
owning extension, and bounded before Ratatui paints it. A reload cancels the
visible request and queued requests before replacement events arrive. Writes
through `ctx.workspace` require explicit consent and are limited to reviewed
working-tree files.

The bottom status row is one host-owned subsystem (`workdeck-tui`'s status-line
store and layout): persistent items in symbolic tones, one inline prompt, and
the keyboard-mode badge. Items are namespaced `ext:<id>:<item>` so two
extensions cannot collide with each other or host items, and a registry
replacement clears them in one sweep; ordinary content reloads keep the same
registry and its items. The layout is deterministic and theme-free — the badge
is never dropped, a prompt takes the left region while open, overflow drops the
lowest-priority item whole before truncating the last survivor, and prompt
lead-ins truncate before the input's minimum width. The store owns the prompt
queue: one visible prompt, FIFO behind it, reload cancels everything except the
host filter's opted-in input, and shutdown settles the rest. Prompts and items
cross the native boundary as declarative actions validated against the
`status-line` capability.

## Events and snapshots

Lifecycle and custom event requests are queued per extension in declaration
order. Event payloads contain owned `ReviewSnapshot` values, not renderer
references; causal depth and response deadlines prevent recursive or stalled
handlers from blocking input. Startup events are emitted only after the
registry commit; reload events identify `watch`, `daemon`, or `manual` origin.

The session broker and CLI expose the same provider-neutral selection, file,
hunk, note, and navigation models. Saved notes preserve source anchors and
parent identities. A snapshot from a retired generation is inert and cannot be
used to navigate, open a dialog, or write a document.

## Ownership map

| Responsibility | Rust owner |
| --- | --- |
| Provider-neutral changesets and identity | `workdeck-core` |
| Git/Jujutsu/Sapling and snapshots | `workdeck-vcs` |
| Patch parsing, alignment, wrapping, syntax spans | `workdeck-diff` |
| Review state, navigation, notes, reloads | `workdeck-review` |
| Ratatui panes, dialogs, routing, painting | `workdeck-tui` |
| Session daemon and authenticated protocol | `workdeck-session` |
| Native SDK and JSON-RPC host | `workdeck-extension-api`, `workdeck-extension-host` |
| Durable entities, events, import/export | `workdeck-store` |
| CLI composition and process ownership | `workdeck-cli` |

The boundary tests in `xtask/src/architecture.rs`, extension protocol tests,
and compiled examples are executable evidence for this map. The old
TypeScript/OpenTUI runtime is retained only in protected Hunk upstream refs and
is never executed or mirrored in the Workdeck tree.

Adapted from Hunk's MIT architecture guide, Copyright Modem Labs Inc.; native
Rust modules and Ratatui replace its in-process JavaScript/OpenTUI host.
