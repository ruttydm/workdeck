# Native process environment migration

Workdeck replaces `scripts/script-helpers.ts` with `xtask/src/process.rs`. Cargo and Git launches
use a Rust-owned environment boundary: all case-insensitive variants of `PATH` are removed and one explicit
value is inserted, while unrelated variables are preserved. This retains the source helper's
cross-platform invariant without shipping Bun, Node, npm, or a package-manager runtime.

The native helper is exercised by executable Rust tests and is available to tooling that needs a
deterministic `Command`. The pinned source is read from both protected Hunk refs for its 1,386
bytes, 37 lines, and SHA-256 `e5997dcba8a94558dede41e70be51503e8faa16020587b20b9c2f33695010424`.
