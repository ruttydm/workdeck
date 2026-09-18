# Hunk session CLI integration migration

This document records the complete semantic-rebase disposition for Hunk's
`test/session/cli.test.ts`. The pinned source is read through `git show`; no
TypeScript source mirror is retained in the Workdeck tree.
No TypeScript source mirror is retained in the final tree.

| pin | bytes | lines | SHA-256 |
| --- | ---: | ---: | --- |
| `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` | 30,060 | 979 | `b6fb57de48bdb94c6736808e8ddb90a1351d589c1ec1df624aac7b997256514c` |
| `4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` | 28,511 | 934 | `d9349c1643ea6d1c15e176dc4434d64d83b2eac8eb135d401c6bdf486f8ea84e` |

The full source interval is covered once, from byte `0` to the pinned byte
length; the coverage is complete and non-overlapping, with no gaps or
overlaps. The verifier also checks the exact
helper, type, interface, constant, and test-name surfaces at both pins. The
stable pin intentionally renames the option-like-range test while preserving
its rejection contract.

## Native ownership

The terminal process and fixture lifecycle are exercised by the native PTY and
lifecycle suites. Session list/get/context, reload, navigation, comments, JSON
and human-readable output are owned by `workdeck-session`'s command runner,
typed HTTP client, formatters, and authenticated loopback broker. Reload confinement is
validated before I/O by `reload_bounds`, and the Ratatui `AppHostController`
commits reloads and live comment focus only after the broker transaction.

The seven pinned integration tests map to executable Rust tests covering:

- daemon-backed list/get/context metadata and empty-daemon behavior;
- replacement reloads and preservation of live comments;
- outside-root and option-like VCS-range rejection;
- navigation and comment focus semantics;
- `--stdin` comment batches with and without `--focus`.

Native tests are referenced by exact `#tests::...` anchors in the ledger and
are checked with `syn` by `cargo xtask verify` and `cargo xtask port audit`.

## Oracle and attribution

Dual-pin oracle evidence is retained in `port/hunk/oracles/session-bootstrap.json`,
`hunk-session-bridge-hook.json`, `app-host-reload.json`,
`app-host-reload-root.json`, and `terminal-review-gap-reload-execution.json`.
These fixtures preserve daemon/session snapshots, reload confinement, focus,
and terminal lifecycle observations from both pins. Hunk's MIT attribution is
retained in `THIRD_PARTY_NOTICES`; all translated test behavior is implemented
in Rust and remains under Workdeck's MIT license.
