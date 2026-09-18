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

## Saved-note contract Clippy refresh

On the worktree based on `fd2b1f24`, including the supplemental alpha draft-anchor
test and two `std::slice::from_ref` corrections in the repeated-comment-reveal
test, strict workspace/all-target Clippy passed in 15.86 seconds. The first run
had rejected the two cloned single-item slices; they were fixed without warning
suppression. This refresh uses the same Rust 1.95.0 macOS arm64 host, two build
jobs and disabled incremental compilation.

This covers the intervening annotated navigation, repeated reveal, save-time IDs,
save-result contract and reply-deletion guard changes. It is not an updated
full-workspace-test, release-build, benchmark or cross-platform result.

## Composer failure-path Clippy refresh

At clean source commit `d86dcb77c24976702074f3516d1797250d33d7a4`, strict
workspace/all-target Clippy passed in 1 minute 23 seconds, with two build jobs and
incremental compilation disabled. This covers keyboard composer reveal, saved
timestamps, retained blank edits, and missing/orphaned/cross-file reply-parent
validation. The cached `target/debug/xtask` executable was absent at the start;
the direct audit invocation therefore failed to launch, not on ledger validation.

`cargo xtask port audit` then rebuilt the executable (1 minute 14 seconds) and
ran without `--allow-incomplete`. It exited 1 at the incomplete-coverage gate:
1,257 files, 1,434 interval records, 467 translated-test records, 277 unmapped
records and 92 pending upstream commits. No fetch was performed. The recent
runtime fixes do not establish full coverage or eliminate the release blockers.

## Measured-cursor and live-projection Clippy refresh

At clean source commit `2330d95a8873ca8059cf43e10b2f40a75a831f03`,
`cargo clippy --workspace --all-targets -- -D warnings` passed in 1 minute
21 seconds on the local macOS arm64 host, with incremental compilation disabled
and two build jobs. This includes the missing-measured-cursor draft reveal fix,
completed live-comment merged-file projection assertions, and catalog-effect
declaration checks. No warning suppression or source change was needed.

This refresh establishes the Clippy gate for that source commit only. The latest
strict source audit still fails with 277 unmapped interval records and 92 cached
pending upstream commits, as recorded in `live-beta-annotated-navigation.md`.
It does not refresh full workspace tests, benchmarks, cross-platform native CI,
upstream fetch state or release-artifact verification.

## Full workspace refresh after startup and scanner fixes

At clean implementation commit `0fc5deeb40a762406851701007db0c90e8ea7e39`,
`cargo test --workspace --all-targets` completed with exit status zero on the
local macOS arm64 host, using two build jobs and disabled incremental compilation.
This run includes the watcher setup-event correction, build-generated syntax set,
and production/test-aware session-boundary scanner.

The run passed 98 terminal pager tests, 127 diff tests, all 18 review-triage tests,
1,255 TUI tests and 242 VCS tests. Tooling passed 245 tests with its existing
oracle-capture test explicitly ignored, followed by passing changelog/catalog CLI,
test-command and theme-probe PTY integration suites. The build phase took 13.19
seconds; suite timings are not benchmark parity measurements.

`cargo deny check` also exited zero: advisories, bans, licenses and sources passed
under the current repository policy. Duplicate-version and unused-license-allowance
warnings remain; this is not a claim of a warning-free dependency graph.

This refresh does not establish full Hunk parity. The latest strict ledger audit
still reports 277 unmapped records and 92 cached pending upstream commits. No
upstream fetch, optimized benchmark, cross-platform native CI or release-signature
verification was performed by these checks.
