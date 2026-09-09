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
The baseline `.env.test` ledger record remains unmapped pending complete test
entry-point coverage and native platform verification. This is not a claim that
the complete workspace passes under the new isolated environment.

At clean `65110d29`, the VCS library passed under both null-config variables:
241 tests, zero failures/ignored/filtered, 132.05 seconds on Darwin arm64.
Native watcher tests took over a minute but completed without restart. This
validates the VCS-library scope only, not the new workflow runs or Windows.
The shared command regression verifies locked workspace/all-targets arguments,
working directory, and both explicit configuration overrides; xtask all-target
Clippy and formatting also pass.
