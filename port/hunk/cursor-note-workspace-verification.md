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
