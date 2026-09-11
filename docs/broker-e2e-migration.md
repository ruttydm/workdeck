# Hunk session-broker end-to-end migration

`test/session/broker-e2e.test.ts` is accounted for as a complete translated
test interval. The verifier reads the pinned blobs directly from Git and
requires the exact public helper, type, interface, constant, and test surfaces
at both pins.

| pin | bytes | lines | SHA-256 |
| --- | ---: | ---: | --- |
| `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` | 23,792 | 790 | `869e91e49edef872c157e7a192e64ea984ffeba5a71e9839507e7fe7788ad04a` |
| `4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` | 23,235 | 779 | `67555ddeb64ce4138faf0fd632a8e0ff93f332379035eb25d782747797b8671a` |

The baseline and stable source intervals are each complete and
non-overlapping. Stable removes only the PID-metadata helper; daemon identity
is intentionally observed through the native authenticated broker lifecycle.

## Native behavior

The Rust projection covers:

- authenticated loopback daemon auto-start and producer registration;
- live session list/context, CLI comment and highlight routing;
- exact line reveal and Ratatui terminal rendering;
- multi-session selector isolation on one daemon;
- graceful teardown and a foreign-listener port conflict.

The native broker adapter corpus exercises both Bun- and Node-compatible wire
semantics without executing either runtime. `workdeck-session` owns the
authenticated protocol and CLI, while `workdeck-tui` owns review publication
and rendering. Every translated source test maps to one or more executable
Rust tests by exact `#anchor`; the `xtask` verifier rejects missing anchors.

Dual-pin oracle evidence is retained in
`port/hunk/oracles/hunk-session-bridge-hook.json`,
`app-host-reload.json`, `pty-session-attention.json`, and
`extension-vcs-patch-result.json`. Hunk's MIT attribution remains in
`THIRD_PARTY_NOTICES`. No TypeScript source mirror is retained in the final
tree.
