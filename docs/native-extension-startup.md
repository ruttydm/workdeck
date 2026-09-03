# Native extension startup

Workdeck's startup loader is the Rust/native counterpart of Hunk's extension startup boundary. It
owns one complete discovery snapshot, the configuration values observed by that snapshot, the
shared notification and stderr-log hubs, every successfully started child, contained load issues,
and any pending repository-trust decision. Every handshake receives the exact session working
directory; the child process itself still starts in its manifest directory so relative executable
assets remain deterministic.

## Loading and continuation

The disabled path returns before discovery or trust lookup, so `--no-extensions` cannot read an
extension manifest or execute code. An enabled pass discovers candidates in this order: explicit
CLI paths, user-configured paths, global extensions and trusted repository extensions. Each group
is deterministic and first-wins namespace ownership is settled before a child starts.

A provisional result may be supplied to a later pass. Workdeck reuses it only when all of these
conditions hold:

- the working directory is unchanged;
- the old candidate list is an exact path-and-origin prefix of the new list;
- the new list is not shorter;
- configuration for every namespace claimed by that prefix is deeply equal.

Only the appended suffix is then validated and started. A changed cwd, candidate, order or loaded
configuration retires the entire earlier pass before replacement loading begins. Retirement first
revokes every retained capability, then gives all children one shared bounded shutdown deadline.
The notification and ordered log hubs are carried across either path so a mounted Ratatui surface
keeps receiving messages and every stderr line retains its extension attribution. Stderr is the
native SDK's reserved logging channel and is never interpreted as JSON-RPC or terminal markup.

## Failure presentation

Manifest, spawn and handshake failures remain contained and source-attributed. Interactive review
turns each issue into a `StartupNotice` with a stable `extension:<manifest-path>` key. The detail is
stripped of terminal control sequences, reduced to its first physical line, trimmed, and capped at
120 characters with an ellipsis. Existing configuration notices remain first. When no extension
failed, `merge_startup_notices` borrows the original slice so an unchanged reload preserves its
allocation and identity.

Ratatui presents startup notices on its ordinary footer row before buffered runtime extension
notifications. Each notice has a complete timed presentation window and never changes review
geometry.

## Executable evidence

`examples/extensions/startup-lifecycle/` is a compiled native fixture. Its integration tests prove
that global configuration and exact cwd reach the handshake, a Unicode stderr log is captured with
the correct extension ID, appending repository discovery does not start the global child twice, and
changed configuration produces `factory:1`, `shutdown`, `factory:2` in that order. The exact
baseline and stable Hunk oracles are `port/hunk/oracles/extension-startup.json` and
`port/hunk/oracles/extension-types.json`.
