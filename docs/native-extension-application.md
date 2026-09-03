# Native extension application and containment

Workdeck applies native extension declarations through one deterministic resolver before the
declarations reach VCS loading or the Ratatui session. This is the Rust counterpart of pinned
Hunk's `src/extensions/apply.ts`; it keeps application policy separate from handshake validation.

The resolver walks extension load order, then declaration order. File-language declarations stay
last-wins within selector category, while Workdeck's built-in `.mts` and `.cts` ownership cannot be
overridden. VCS adapter IDs, qualified pane IDs, pane replacement targets, qualified file-view IDs,
qualified line-highlighter IDs, qualified keyboard-mode IDs, and `<extension>.<command>` IDs are
first-wins. Every later claimant is removed from the live registry and retained as an attributed
application issue.

The CLI uses the accepted VCS declarations to build the provider-neutral catalog before initial
loading. The TUI uses the same accepted declaration addresses for panes, commands, modes, views,
highlighters, and syntax mappings. Fresh-launch issues become terminal-safe `StartupNotice` values;
trust-triggered in-session replacement sends the same messages through the warning notification
surface. Language mappings are rebuilt from the complete accepted set on every load, so a selector
that disappears cannot leak into a later review.

Changeset transforms execute in extension and declaration order. Every request receives an owned
copy of the previous accepted changeset. Transport failures, timeouts, malformed typed responses,
empty file IDs, and duplicate file IDs produce attributed warnings and retain that previous value;
later transforms still run. A valid transform may filter or reorder files, after which Workdeck
refreshes provider-neutral review identities before rendering.

Native declarations distinguish the closed lifecycle namespace from the open extension bus.
`EventSubscription` accepts only the 15 pinned lifecycle names. `CustomEventSubscription` accepts
any nonblank name, including names with spaces or Unicode, and `PendingCustomEvent` retains factory
emissions until every extension has registered. Typed Rust declarations make JavaScript values such
as non-functions unrepresentable; malformed raw JSON fails the entire handshake atomically, which
is the native-process equivalent of Hunk rolling a failed factory back.

Executable parity evidence lives in `port/hunk/oracles/extension-application.json`. It records the
baseline and stable blobs, the 60/60 and 55/55 oracle runs, every source-test mapping, the five
baseline-only selector tests, and the Rust commands that exercise the application boundary.
