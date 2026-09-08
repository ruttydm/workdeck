# Architecture

Workdeck is a Rust workspace with one production composition root and one shipped executable. Run
`cargo xtask architecture check` to validate these boundaries against the live Cargo metadata and
Rust module tree. `cargo xtask verify` runs the same check before its build and smoke gates.

See the [source ownership map](source-architecture.md) for the complete pinned
source-role migration, bootstrap invariant, and incremental migration policy.

## Crate ownership

| Crate | May depend on these Workdeck crates |
| --- | --- |
| `workdeck-core` | none |
| `workdeck-diff` | core |
| `workdeck-extension-api` | core |
| `workdeck-review` | core, extension API |
| `workdeck-vcs` | core, diff |
| `workdeck-extension-host` | core, diff, extension API, review, VCS |
| `workdeck-session` | core, diff, review, VCS |
| `workdeck-markup` | none |
| `workdeck-migration` | none |
| `workdeck-store` | none |
| `workdeck-tui` | core, diff, extension API and host, markup, review, session, VCS |
| `workdeck-cli` | every product crate; this is the composition root |

The allowed lists are ceilings, not required edges. Removing a dependency does not require a
baseline update. Adding an edge outside the table fails with the owning boundary name. Cargo already
rejects dependency cycles, and the checker independently walks the workspace graph so the invariant
and its failure remain executable evidence.

`workdeck-cli` must expose exactly one production binary named `workdeck`. `xtask` and the compiled
native extension examples are non-publishable repository tools and fixtures, not shipped product
executables.

## Source boundaries

The checker also validates invariants that a crate graph alone cannot express:

- every Rust file below each product crate's `src/` directory, and below `xtask/src/`, must be
  reachable from a library or binary module root; `#[cfg(test)]` module trees are tracked separately
  and cannot hide an orphan;
- only `workdeck-tui`'s composition shell, current-review controller, refresh controller, and review
  state adapter may import `workdeck-session`;
- native extension implementation files consume declarative extension API data and actions and may
  not import the CLI, session broker, or TUI renderer;
- native extension declarations, loaded metadata, context cwd, notification/log hubs, provisional
  factory events, and runtime authority cross one explicit native type boundary (see
  [Native extension type boundary](native-extension-type-boundary.md));
- native extension startup resolves only a canonical `workdeck-extension.toml` boundary and its
  named compiled executable; adjacent JavaScript package metadata and source modules are never
  runtime inputs; candidate namespaces are settled before startup and each failed process is
  isolated from the remaining load pass (see
  [Native extension runtime boundary](native-extension-runtime-boundary.md));
- startup passes reuse only an unchanged cwd, candidate/config prefix, retire incompatible passes
  before replacement startup, send the exact session cwd to each factory, capture attributed stderr
  logs, and surface bounded terminal-safe failures through the Ratatui
  footer (see [Native extension startup](native-extension-startup.md));
- extension CLI commands use lazy stdin and ordered output leases over the host-owned JSON-RPC
  channel, and cannot retain terminal or signal authority after settlement (see
  [Native extension CLI runtime](native-extension-cli-runtime.md));
- extension lifecycle and custom events use owned snapshots, chronological per-process queues,
  factory-event replay after all registrations, nonblocking Ratatui polling, atomic revocation, and
  one bounded retirement window (see
  [Native extension events and pane actions](native-extension-events-and-pane-actions.md));
- extension VCS adapters are declared in the atomic handshake, translated by the host into the
  provider-neutral catalog, and share one serialized native process across initial load, exact
  source reads, watch callbacks, reloads, and the TUI; adapter-returned roots remain authoritative
  for the review session (see
  [Native extension VCS adapters](native-extension-vcs-adapters.md));
- the semantic review reducer is crate-internal and is reached through intents and
  `SemanticReviewStore` dispatch;
- semantic notes cross into terminal-local file ids, line coordinates, draft shapes, and thread
  guides only through `workdeck-review::review_note_mapping`; the projection preserves every
  annotation field and uses the shared anchor and visible-thread selectors rather than deriving
  ownership or nesting in a renderer;
- `workdeck-diff` implementation modules remain private; only deliberate facade items are
  re-exported.

These checks are the Rust adaptation of Hunk's pinned dependency-cruiser policy. The CLI is the Rust
composition root, so it intentionally owns the TUI rather than preserving Hunk's React-specific
`src/app`/`src/ui` import direction. Similarly, the native extension API may use provider-neutral
core value types instead of duplicating them at the protocol boundary.

## Shrink-only baseline

The pinned Hunk violation file is exactly an empty JSON array. Its native equivalent is an explicit
empty `KNOWN_ARCHITECTURE_VIOLATIONS` constant. The checker fails both for an unexpected violation
and for a stale known violation that has been fixed. There is no command to append exceptions or
weaken the baseline.
