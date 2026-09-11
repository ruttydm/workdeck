# PTY harness semantic migration

The pinned Hunk `test/pty/harness.ts` helper is accounted for as a complete test-support source
contract.  The baseline blob at `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` is 36,240 bytes
(SHA-256 `83653303ff81ef7ba00a5bacb3eab2832ce8fc8b4f87c376d0d1eed37d834b51`).  The stable
`v0.20.1` blob at `4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` is 35,035 bytes (SHA-256
`1f19ec9777ac4cc3487158e3c7b78bfd6008bc18ad702342ad9912f5e084fd10`).  The verifier reads
both blobs through Git, enumerates every exported/private top-level helper and `ChangedFileSpec`,
and requires complete non-overlapping coverage.  No TypeScript source mirror is retained.

## Native test boundary

The Rust equivalent uses `qwertty-term-vt` for terminal cell capture and a real Unix PTY for the
Workdeck subprocess.  `crates/workdeck-cli/tests/terminal_pager.rs` owns launch, raw input,
wait/predicate, resizing, mouse, and cleanup semantics; its `terminal_pager/harness.rs` owns
string projections, repository fixtures, fixture file pairs, Git repository setup, and five-step mouse interpolation.
The lifecycle suite covers controlling-terminal teardown and signal behavior.

The port preserves the helper's observable contracts: isolated configuration, deterministic
numbered fixtures, shell-safe Git invocation, UTF-16/source-column lookup, cell-background
inspection, add-note affordance probing, wrapped/cross-file scrolling, source-gap expansion,
notes, and agent-highlight sessions.  Rust tests execute these behaviors against frozen
baseline/stable oracles rather than launching Bun, Tuistory, or a JavaScript runtime.

Focused verification is:

```text
cargo test --locked -p xtask native_rust_pty_harness_replaces_both_pinned_sources -- --nocapture
```
