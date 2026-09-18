# Git configuration isolation in verification

`cargo xtask test` and the test stage of `cargo xtask verify` run their Cargo workspace-test child with
`GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` pointing at the platform null device
(`/dev/null` on Unix, `NUL` on Windows). This translates the intent of pinned
Hunk's two-line `.env.test` for this verification entry point. Other verification
steps and ordinary Workdeck commands retain their normal configuration behavior.
No user configuration file is modified and no process-global environment mutation
is performed. Tests can still deliberately override configuration for a child
Git process to exercise configuration-dependent behavior.

CI and release workflows invoke `cargo xtask test`, which accepts no arguments
and retains `cargo test --locked --workspace --all-targets` as its full scope.
The executable regression supplies a real temporary configuration independently
as global and system configuration, verifies Git can read its probe, applies the
test environment, verifies the probe is absent, and checks the original file is
unchanged. Direct `cargo test` does not receive this wrapper automatically.
The baseline `.env.test` record is mapped to this native wrapper and its
cross-platform null-device test. Full workspace parity and Windows execution
remain separate release gates.

At clean `65110d29`, the VCS library passed under both null-config variables:
241 tests, zero failures/ignored/filtered, 132.05 seconds on Darwin arm64.
Native watcher tests took over a minute but completed without restart. This
validates the VCS-library scope only, not the new workflow runs or Windows.
The shared command regression verifies locked workspace/all-targets arguments,
working directory, and both explicit configuration overrides; xtask all-target
Clippy and formatting also pass.

`xtask/tests/test_cli.rs` additionally executes the built tool with unsupported
arguments from an empty non-repository directory and a PATH without Cargo or Git.
All four cases report `test accepts no arguments`, exit 1, emit no stdout and
create no files. This confirms validation happens before repository discovery or
test spawning; it does not exercise a successful whole-workspace child run.

The complete `CARGO_INCREMENTAL=0 cargo xtask test` run subsequently exited 0 at
clean `de047d8ffba009f26c0793e452b15b625389ccf0` on Darwin arm64. The child retained
the locked workspace/all-targets scope. The xtask unit suite explicitly ignored
`ci_changes::tests::capture_pinned_ci_change_oracles` (an opt-in source-oracle
capture helper); this is not a claim that every declared test executed.
See [the checkpoint evidence](../port/hunk/verification-de047d8f.json).

During that run, the VCS suite took 269.46 seconds and passed all 241 tests.
A one-second native stack sample during the delay showed notify FSEvents workers
waiting in `FSEventsGetCurrentEventId` and `FSEventStreamStart` RPCs, registration
waiting on a receiver, and teardown joining watcher threads. These waits cleared
without restarting the process. The sample identifies the observed wait location,
not an established root cause or a watcher latency acceptance result.
