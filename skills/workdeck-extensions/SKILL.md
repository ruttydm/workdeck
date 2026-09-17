---
name: workdeck-extensions
description: Build, debug, validate, install, or change trusted native Workdeck extensions, including commands, CLI commands, panes, file views, keyboard modes, line highlighters, themes, syntax mappings, changeset transforms, VCS adapters, lifecycle events, dialogs, review navigation, configuration, and guarded workspace access.
---

# Building Workdeck extensions

A Workdeck extension is a compiled executable beside a `workdeck-extension.toml` manifest. The
host starts it as a trusted subprocess and exchanges JSON-RPC 2.0 messages, one JSON object per
line. Workdeck retains terminal rendering and input ownership; the extension returns typed
registrations, declarative views, and host actions.

Start from the nearest complete example instead of inventing a protocol loop:

```sh
cargo xtask extension stage-example cli-tools
workdeck --extension target/workdeck-extension-examples/cli-tools cli-tools status
```

A minimal manifest has this shape:

```toml
id = "acme.review-tools"
name = "Acme review tools"
version = "0.1.0"
api_version = 1
executable = "bin/acme-review-tools"
capabilities = ["commands", "notifications"]
description = "Review commands for Acme repositories"
```

Request only the capabilities the extension uses. Its first host request is
`workdeck/handshake`; return `HandshakeResponse { extension_api_version: API_VERSION, ... }` with
the complete registration set. Registration publication is atomic: one invalid declaration rejects
the whole handshake.

## Sources of truth — read before writing

| Source | What it answers |
| --- | --- |
| `crates/workdeck-extension-api/src/lib.rs` | API version, manifest, capabilities, registrations, requests, actions, limits, and wire types. |
| `crates/workdeck-extension-api/src/authoring.rs` | Provider-neutral changeset, lifecycle, note, review, and workspace authoring models. |
| `crates/workdeck-extension-api/src/file_views.rs`, `panes.rs`, `vcs.rs`, `keys.rs` | Exact file-view, pane, VCS, and chord contracts. |
| `docs/native-extension-runtime-boundary.md` | Manifest resolution, process lifetime, protocol isolation, and trust. |
| `docs/native-extension-events-and-pane-actions.md` | Commands, dialogs, navigation, events, pane actions, and keyboard ownership. |
| `docs/native-interactive-file-views.md`, `docs/file-view-native-components.md` | Deterministic layouts, interactive modes, fixed-height components, and guarded writes. |
| `docs/native-extension-vcs-adapters.md` | Detection, load, exact sources, caching, and watch operations. |
| `examples/extensions/*` | Compiled reference implementations and manifests. Copy their protocol and validation patterns. |

Read the focused native boundary documents above only when changing Workdeck itself. For a user
extension, the public API crate and examples are the contract.

The examples demonstrate these surfaces:

- `cli-tools/` — a generic top-level command, byte-exact output, lazy stdin, cancellation, and
  one-time delegation to a built-in review command;
- `github-pr/` — authenticated HTTP, private temporary artifacts, cleanup on shutdown, and
  delegation to `workdeck patch`;
- `review-triage/` — panes, clickable rows, commands, three dialog shapes, lifecycle events,
  notifications, review navigation, and the extension event bus;
- `inline-edit/` — an interactive file-view mode with source-aware editing and consented writes;
- `rendered-markdown/` — host-rendered rows derived from parsed content;
- `jsx-file-view/` and `file-view-gallery/` — fixed-height declarative row components and
  responsive layouts;
- `review-note-navigator/` and `review-snapshot-export/` — authoritative saved-note snapshots;
- `vim-navigation/` — counts, chords, a session keyboard mode, and public command execution;
- `native-vcs/` — every detection, load, source-read, cache, and watch method;
- `startup-lifecycle/` — configuration, stderr logging, continuation, replacement, and shutdown.

## Where extensions live

| Source | Trust behavior |
| --- | --- |
| repeatable `--extension <path>` | Explicit user intent; starts immediately. |
| `[extensions] paths` in `~/.config/workdeck/config.toml` | User-owned configuration; starts immediately. |
| `~/.config/workdeck/extensions/` | Global discovery; starts immediately. |
| managed installs below `~/.config/workdeck/extensions/installed/` | Global discovery after an explicit install. |
| `.workdeck/extensions/` or repository-configured paths | Omitted until the repository trust decision is allowed. |

A path may name a manifest, a directory containing one manifest, or a directory whose immediate
children contain manifests. Relative user-config paths resolve from the invocation directory;
repository-config paths resolve from the repository root. Canonical path identity removes duplicate
aliases. Discovery order is explicit paths, user-configured paths, global paths, then trusted
repository paths; compatible extension IDs are first-wins.

The repository-root `.workdeck/extensions/` directory is the canonical automatic source.
Before a native `.workdeck/` root exists, discovery can read an existing
`.agents/workdeck/extensions/` directory for compatibility. Creating the native root stops
that automatic legacy discovery; migrate the extensions or configure their existing paths
explicitly. Discovery never copies or rewrites the legacy directory. Repository trust still
applies to the selected directory and repository-configured paths.

An explicit path is consent even when it points inside the repository. Read the manifest and the
code that produced its binary before suggesting or executing it. Native extensions run with the
user's full permissions. Deadlines and crash containment protect Workdeck's lifecycle; they are not
a security sandbox.

Install and manage shared extensions with:

```sh
workdeck extension install <owner/repo[@ref] | git-url[@ref] | local-path> [--yes]
workdeck extension list
workdeck extension update [managed-name]
workdeck extension remove <managed-name>
workdeck extension validate <manifest-or-directory>
workdeck extension trust --repo <path> --allow --yes
workdeck extension trust --repo <path> --deny --yes
```

Managed repositories must place a valid manifest at their root and include the compiled executable
at its safe relative manifest path. Tag immutable releases when consumers need `@ref` pins. The
manager clones into the global installed directory, records the exact commit, stages updates before
promotion, and never builds downloaded source during discovery.

Manifest IDs start with an ASCII letter or digit and then contain only ASCII letters, digits,
dots, dashes, or underscores. `workdeck`, `git`, `jj`, and `sl` are reserved. The ID owns the
namespace: commands become `<extension>.<command>`, while panes, file views, keyboard modes, and
line highlighters become `<extension>:<local-id>`. Bad or duplicate IDs are skipped with an
attributed startup notice.

## Pick the touchpoint

| To do this | Handshake registration or response action |
| --- | --- |
| Keep demo or training view settings temporary | `Registration::SessionOptions` with transient view preferences. |
| Add a selectable color theme | `Registration::Theme`. |
| Highlight an extension, exact filename, or filename glob | `Registration::FileLanguage`. |
| Support another VCS | `Registration::VcsAdapter` plus the `workdeck/vcs/*` methods. |
| Add a navigation, list, or status pane | `Registration::Pane` plus pane render/action methods. |
| Present a file as something other than a raw diff | `Registration::FileView` plus match/layout methods. |
| Mark character ranges inside diff lines | `Registration::LineHighlighter`. |
| Interpret review keys as a temporary global mode | `Registration::KeyboardMode`. |
| Add a generic top-level CLI command tree | `Registration::CliCommand`. |
| Bind a review key or add an Extensions-menu entry | `Registration::Command`. |
| Hide, reorder, or retitle files before review | `Registration::ChangesetTransform`. |
| Respond to review lifecycle changes | `Registration::EventSubscription`. |
| Coordinate with another loaded extension | `CustomEventSubscription`, `PendingCustomEvent`, and `EmitEvent`. |
| Read user-supplied settings | `HandshakeRequest.config` after requesting `configuration`. |
| Read stable files and saved notes | the invocation's immutable `review` and `workspace` snapshots. |
| Navigate or open UI without owning the terminal | return validated `ExtensionHostAction` values. |

`API_VERSION` is currently 1. Compile against `workdeck-extension-api`, put the same version in the
manifest, and echo it from the handshake. An incompatible host or extension refuses startup before
any registration becomes visible.

### Generic CLI handlers

Register one lowercase-kebab top-level token. Built-ins and aliases cannot be shadowed, and the
first extension claim wins. Put an explicit extension before its command during development:

```sh
workdeck --extension ./target/acme-review-tools acme-sync status --help
```

`workdeck/cli/invoke` receives immutable raw args and the invocation cwd. The extension can send
`workdeck/cli/output` notifications for byte-exact stdout or stderr and lazily request bounded stdin
chunks. Return an exit code or one built-in delegation argv. Delegation is one-time: do not write
stdout or begin reading stdin before delegating; use stderr for preparation progress and respond to
cancellation promptly. Reading stdin is an exit-only workflow.

Keep `summary` and `usage` to one terminal-safe line because Workdeck includes them in unknown-
command help. Use `examples/extensions/github-pr/` for a complete preprocessor whose temporary
patch remains valid until delegated review shutdown.

## What handlers receive

The subprocess receives owned Serde values. Mutating its deserialized copy cannot mutate the live
review or another extension's callback state.

- The handshake receives host API/version, extension ID, exact session cwd, granted capabilities,
  and the merged `[extension.<id>]` configuration.
- `workdeck/event` receives the lifecycle name, ordinary snapshot, semantic review snapshot when
  available, event payload, cwd, and currently open panes. `sidebars` remains an exact deprecated
  state alias for `panes`.
- `workdeck/command/invoke` receives the command ID, snapshot, frozen file/hunk/source selection,
  cwd, semantic review snapshot, open panes, active keyboard mode, workspace snapshot, and enabled
  public Workdeck command IDs.
- `workdeck/pane/render` receives placement, exact width and height, semantic theme, and review
  snapshot. `workdeck/pane/action` adds the clicked action ID and fresh review context.
  `workdeck/pane/available` is the synchronous availability probe.
- `workdeck/file-view/matches` receives one frozen file. `workdeck/file-view/layout` adds the
  width, change ranges, exact old/new source snapshots, and cancellation state.
- File-view and session keyboard-mode enter/key/exit requests carry activation-scoped review and
  command state. Key responses decide `handled`, `pass`, or `exit` and may include host actions.
- VCS requests carry only provider-neutral detection, review-input, exact-source, and watch data;
  renderer choices never cross into a backend.

Panes and file views return declarative `ViewNode` or row trees. They never return terminal escape
sequences or host widget objects. Ratatui validates sizes and resource limits, measures geometry,
renders every cell, records click targets, and owns all modals.

## Rules that bite

Most extension bugs are one of these:

- **Registering a surface does not show it.** A pane needs `default_open`, a replacement target, or
  a command action that opens it. A file view remains raw until selected or activated.
- **A rejected file-view layout becomes the raw diff.** Return one inclusive hunk-row range per
  parsed hunk in matching order; keep row IDs stable and source ranges non-overlapping on each side.
  Invalid, oversized, cancelled, timed-out, or crashing layouts warn once and fall back.
- **The host is the only renderer.** Build `ViewNode` trees and `ExtensionFileViewRow` values. Do
  not emit terminal controls, instantiate another renderer, or assume a component framework.
- **Layout is a pure derivation of the request.** Store durable state in the subprocess, then
  return `RefreshFileView` or `RefreshPane` after a state change. Scope refreshes by file ID where
  state belongs to one file.
- **Use the semantic review snapshot for complete saved-note state.** Note events are incremental,
  drafts are excluded, and stale or orphaned saved notes can still exist. Re-read the snapshot
  before irreversible async work and compare its generation/revision identity.
- **Retained review authority expires on reload.** Stale responses cannot navigate replacement
  content, settle retired dialogs, or start a write. A consented write already in progress reports
  its real result; shutdown is only for extension-owned cleanup.
- **A soft review reload normally keeps the process.** File IDs can change when file order changes,
  so key durable per-file state by path or reconcile it on `changeset_loaded`. A changed extension
  set, cwd, configuration snapshot, or trust state can replace the process entirely.
- **Transforms must retain a valid provider-neutral changeset.** Preserve renderer metadata, keep
  file IDs unique, and return the complete new value. Failure retains the previous accepted
  changeset so later transforms can still run.
- **Chords are defaults.** Users can remap by qualified command ID. Built-ins and earlier accepted
  claims win individual conflicts; a command with no remaining chord still appears in the menu.
  Bind the character produced by Shift, such as `!`, rather than a physical-key description.
- **Keyboard modes own grammar, not rendering.** Keep counts and pending sequences in the
  extension, then execute one enabled public command. Dialogs, focused host inputs, and file-view
  modes outrank a session mode; Escape and the status/menu exits remain host-owned.
- **Public command execution is guarded twice.** Probe the invocation's enabled IDs, return an
  execution action with a positive count no greater than 10,000, and expect stale or disabled
  actions to return false or be discarded.
- **Treat configuration as untrusted data.** Repository configuration can override settings for a
  globally installed extension. Validate anything used as a path, command, endpoint, or process
  argument.
- **Workspace writes are narrow and consented.** They apply only to reloadable unstaged
  working-tree reviews, exact reviewed file IDs, unchanged source snapshots, and paths contained by
  the review root. Check the workspace snapshot before requesting a write and handle written,
  cancelled, and failed results.
- **Visible note placement is all-or-raw per file.** If a visible note cannot bind to one exact
  preferred-side source range, Workdeck renders the whole file as a raw diff rather than guessing.
  Draft note editing is always raw-only.
- **Failures are contained, not sandboxed.** Invalid handshakes publish no registrations. Handler
  failures are attributed, timed out, and quarantined, but the native process still has the user's
  operating-system permissions.
- **Stdout is protocol-only and stderr is logs.** Write exactly one JSON-RPC message per stdout
  line. Never print diagnostics there. Use stderr for logs and notifications for user-facing
  messages.
- **Structured user errors survive the boundary.** Return a JSON-RPC error with a sanitized message
  and optional string `suggestions`; do not leak tokens, response bodies, or control sequences.

## Verifying

Do not seize the user's terminal to test an interactive review. Use this order:

1. Format, lint, and unit-test the extension crate with Cargo.
2. Validate the staged manifest with `workdeck extension validate <path>`.
3. Run protocol tests against buffered stdin/stdout, including malformed JSON, wrong IDs,
   oversize messages, cancellation, deadlines, and shutdown.
4. In a Workdeck checkout, stage the nearest reference with `cargo xtask extension stage-example
   <name>` and run its matching integration test under `workdeck-examples`.
5. For rendering, use the repository PTY/cell-buffer golden harness or a disposable terminal. Test
   representative widths, Unicode, mouse targets, raw fallback, reload, and process failure.
6. Use `--no-extensions` to confirm a symptom belongs to a user extension. Bundled providers and
   the built-in Files pane remain available.

Repository-wide checks are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo xtask verify
```

## If it does not load

- No notice can mean successful but unopened registration, disabled discovery, or no manifest at
  the resolved path. Run `workdeck extension list`, validate the manifest, and inspect discovery
  order.
- A notice naming the extension means an invalid/reserved/duplicate ID, unsafe executable path,
  API mismatch, spawn failure, malformed protocol response, handshake timeout, capability mismatch,
  or invalid atomic registration.
- A repository extension absent from the list is normally awaiting trust or denied. Record an
  explicit decision with `workdeck extension trust` after reviewing it.
- A pane that closes or a file view that returns to raw diff failed validation, timed out, crashed,
  or was quarantined. Check the attributed stderr logs and startup/runtime notice.
- A pane or file view that never appears may simply be closed, unmatched, unavailable, or not
  selected. Registering it does not activate it.
- A command that never fires may have lost its chord to a built-in or earlier extension. It remains
  reachable in the Extensions menu and bindable by qualified ID when registration succeeded.
- A managed extension reported missing on disk must be reinstalled or removed; Workdeck will not
  silently execute a neighboring file.

## Changing Workdeck itself

Only use this section when changing the host or bundled functionality:

- Bundled VCS providers and the built-in Files pane exercise the same public extension model. If
  the public contract cannot express a required bundled behavior, treat that as an API gap rather
  than adding an unowned renderer shortcut.
- Keep author-facing serializable declarations in `workdeck-extension-api`; keep discovery,
  subprocess, protocol, deadline, and authority mechanics in `workdeck-extension-host`; keep
  terminal state and rendering in `workdeck-tui`.
- New surface requires the API type, capability enforcement, atomic registration validation,
  runtime request/response implementation, Ratatui application, failure tests, a compiled example,
  focused architecture documentation, and updated semantic-port oracle/ledger evidence.
- Preserve first-wins ordering, lifecycle event order, stale-authority revocation, strict message
  limits, terminal sanitization, and raw-diff fallback. These are observable compatibility rules.
- Run architecture checks and every extension/example integration test before changing the public
  API version. A version bump needs migration documentation and compatibility evidence.
