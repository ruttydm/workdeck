# Terminal theme probe migration

Workdeck replaces Hunk's `scripts/probe-terminal-theme.ts` with the native
`cargo xtask themes probe` command. The pinned baseline and stable-v0.20.1
source are identical: 1,158 bytes, 46 lines, and SHA-256
`5dd31f87c16c24a0fb64b298d8bd5012e0998d75c03a6c527ef38fa5342cac0e`.

The Rust probe opens the controlling terminal (using `CONOUT$` on Windows or
`/dev/tty` when stdout is redirected), sends the OSC 11 background query,
waits up to 500ms, parses fragmented RGB/hex responses, classifies light or
dark mode, and writes the structured report to stderr while preserving the
stdout/stderr TTY distinction. Crossterm raw-mode transitions are restored on
success and every read, write, resume, or flush failure.

`xtask/src/theme_probe.rs` owns the CLI adapter and recording wrapper;
`workdeck-tui` owns the provider-neutral OSC 11 parser and terminal input
contract. The verifier reads both protected Hunk blobs through `git show`,
checks their exact bytes and required source surface, and requires the Rust
error, fragmentation, timeout, and native probe tests before the ledger record
is considered mapped. No Bun process or TypeScript mirror is retained.
