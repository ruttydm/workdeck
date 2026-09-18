# Darwin child-pipe test isolation

A full `cargo xtask verify` run after `b12d26c1` passed the CLI integration,
review-conformance, nine terminal-lifecycle, and 98 PTY/pager tests, then failed
in the extension-host unit suite: the Darwin closed-child pipe test received
`Ok(())` instead of `BrokenPipe` after waiting for `/usr/bin/true` to exit.
The same test binary passed when that single test was run alone.

This is consistent with another concurrently spawned child temporarily inheriting
a reader before exec. Waiting for the owned child does not establish that every
inherited read descriptor in the process tree has closed. The test now runs its
original body in a fresh single-test process, where no other test can spawn a
child during pipe creation. It still asserts the per-descriptor
`F_GETNOSIGPIPE` setting and the exact `BrokenPipe` result; neither is retried or
relaxed. A failing subprocess propagates its status and captured diagnostics.

Production pipe configuration, write budgets, signal dispositions, and error
handling are unchanged. This is test isolation, not a new runtime port or ledger
mapping. The extension-host library passed all 208 tests after the change and
then passed ten consecutive full runs of the rebuilt test binary.

The subsequent full `cargo xtask verify` run passed: static asset/skill/history
and architecture checks, formatting, workspace tests, strict workspace Clippy,
the optimized `workdeck` build, and large-repository smoke. This includes 98
PTY/pager tests, 208 extension-host unit tests, and 1,181 TUI unit tests. The
tooling suite reports 244 passed and one ignored by default; the ignored
`ci_changes::tests::capture_pinned_ci_change_oracles` was then explicitly run
against both pins and passed separately (one test, 244 filtered out).

This is local macOS validation, not source parity or a stable-release pass.
Strict port audit was not rerun for this test-only change; its latest result
remains 271 unmapped intervals and 11 cached upstream commits. Dependency audits,
same-host performance parity, other native platforms, signing/provenance, and
all remaining source mappings remain independent gates. The clean optimized
build took 9m 11s while another build was active on the host; that duration is
not a launch, reload, navigation, rendering, or memory benchmark.
