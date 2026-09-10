# Cursor and note workspace verification

At source commit `bcba27a0`, `cargo xtask verify` completed successfully on the
local macOS arm64 host after the cursor-navigation tests and explicit pointer-note
target fix. Invocation used Rust 1.95.0, `CARGO_INCREMENTAL=0` and
`CARGO_BUILD_JOBS=2`.

The verifier checked 67 theme assets and notices, generated skills, architecture
boundaries (12 production crates and one shipped executable), all 77 release
fragments and generated history, formatting, workspace tests, strict workspace
Clippy, the optimized Workdeck executable and the large-repository smoke test.
The terminal pager suite passed all 98 tests. Tooling reported 244 passing tests
and one explicitly ignored oracle-capture test. Clippy completed in 17.85 seconds;
the optimized build completed in 62 seconds. These are verification timings, not
same-host source/native performance-parity measurements.

A separate strict `target/debug/xtask port audit` still exited 1: 1,257 files,
1,407 records, 273 unmapped intervals and 11 cached pending upstream commits.
No upstream fetch was performed by this verification. The successful workspace
gate does not establish complete source coverage, full oracle parity, native
other-platform CI, performance parity, or release artifact/signature compliance.
The full semantic-rebase goal remains incomplete.

## Subsequent strict Clippy refresh

At `62b71f62`, after the subsequent session reveal fixes, watcher-test correction
and attention-mark translations, `cargo clippy --workspace --all-targets -- -D warnings`
passed on the same macOS arm64 host using Rust 1.95.0, incremental compilation
disabled and two build jobs. It completed in 83 seconds. The worktree was clean
before the check. This refresh covers Clippy only; it does not update the earlier
full-workspace-test, optimized-build or smoke-test evidence to this newer commit.

## Reload and command-routing Clippy refresh

At clean source commit `5278f5bd6644a1e4d05f71622acf50c70cd96311`,
`cargo clippy --workspace --all-targets -- -D warnings` passed on Darwin arm64
with rustc 1.95.0 (`59807616e`), Cargo 1.95.0, `CARGO_INCREMENTAL=0` and
`CARGO_BUILD_JOBS=2`. It completed in 1 minute 22 seconds. This covers the
recent host-reader retention, forward-gap selection, catalog-driven action
dispatch, stored-hunk reload clamping and supplementary regression tests.

A concurrent strict `target/debug/xtask port audit` exited 1 after validating
ledger structure and evidence: 1,257 files, 1,427 records, 461 translated-test
records, 276 unmapped records and 92 pending upstream commits. No fetch was
performed during this refresh. The earlier 11-commit count above is historical.

This refresh establishes Clippy only. It does not refresh the full workspace
test, optimized-build or smoke-test results, nor establish source parity,
benchmark compliance, other-platform native CI or release-artifact compliance.
No source-ledger disposition changed; the full goal remains incomplete.
