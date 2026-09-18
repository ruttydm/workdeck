# `App` semantic migration

The pinned Hunk application composition is accounted for as a complete source contract.  The
baseline `src/ui/App.tsx` blob at `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` is 57,514 bytes
(SHA-256 `76ed422be18d500c905f7f1f5508a222bdef676f9433a51623527bc17fdb3912`).  The stable
`v0.20.1` blob at `4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` is 100,153 bytes (SHA-256
`514ed6c4591cdc206479664711beea8ff0079a91604722962f162ab7954703ed`).  The verifier reads
both Git blobs directly, checks every top-level function/type/constant/interface, and requires
complete non-overlapping coverage.  No TypeScript source mirror is retained.

## Ownership map

The React `App` boundary is implemented by the native Ratatui shell:

- `ReviewApp` owns the mounted review state, input routing, render loop, themes, cursor/scroll
  geometry, notes, copy actions, and pane output.
- `AppHostController` owns authenticated session attachment, bounded FIFO command processing,
  snapshot publication, reload commit ordering, retirement, and shutdown behavior.
- Extension pane, command, dialog, navigation, workspace-write, theme, and keyboard controllers
  keep their own typed boundaries.  Failures are reported through the host and cannot mutate a
  retired review.
- `workdeck-review` and `workdeck-session` provide the shared semantic store, review producer,
  extension/session snapshots, and provider-neutral command contracts consumed by both keyboard
  input and daemon requests.

The projection preserves Hunk's file filtering, hunk selection, line wrapping, responsive split /
stack layouts, menu and help dialogs, theme preview, agent notes, extension panes and controls,
workspace writes, watch/reload flow, and live-session publication while rendering through Ratatui.
Stable-only workspace-write and selection-debounce helpers are represented by the native host
controllers and their tests rather than by compatibility functions.

## Evidence

The verifier requires executable tests for owner-thread dispatch, FIFO reload and retirement,
view-preference preservation, mouse/keyboard/scroll selection, horizontal clamping, notes and
line highlights, extension panes/commands/dialogs/navigation, and both-pinned AppHost fixtures.
All listed app-host oracles must identify both source pins.  Focused verification is:

```text
cargo test --locked -p xtask native_ratatui_app_shell_replaces_both_pinned_app_contracts -- --nocapture
```
