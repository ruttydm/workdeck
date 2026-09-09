# Git configuration isolation in verification

`cargo xtask verify` runs its Cargo workspace-test child with
`GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` pointing at the platform null device
(`/dev/null` on Unix, `NUL` on Windows). This translates the intent of pinned
Hunk's two-line `.env.test` for this verification entry point. Other verification
steps and ordinary Workdeck commands retain their normal configuration behavior.
No user configuration file is modified and no process-global environment mutation
is performed. Tests can still deliberately override configuration for a child
Git process to exercise configuration-dependent behavior.

The executable regression supplies a real temporary configuration independently
as global and system configuration, verifies Git can read its probe, applies the
test environment, verifies the probe is absent, and checks the original file is
unchanged. Direct `cargo test` does not receive this wrapper automatically.
The baseline `.env.test` ledger record remains unmapped pending complete test
entry-point coverage and native platform verification. This is not a claim that
the complete workspace passes under the new isolated environment.
