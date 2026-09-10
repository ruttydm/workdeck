# Pinned installer platform oracle

`install-platform-oracle.json` contains ten cases each from
`hunk-port/main-2c00f435` and `hunk-port/stable-v0.20.1`. Capture reads each
`install.sh` using `git show`, extracts its original `fail`, `detect_os` and
`detect_arch` shell functions, and invokes them through `sh -c`. Only `uname`
and `sysctl` are replaced with deterministic input providers. The installer main
function is never executed: no download or installation occurs. No shell source
mirror is committed.

Cases cover Darwin x86_64 with/without Rosetta, translated amd64, Darwin aarch64,
Linux x86_64/amd64/arm64/aarch64, unsupported riscv64 and unsupported FreeBSD.
Each record contains the pin, supplied platform facts, exit code, separate stdout
and stderr, and their concatenation retained as `output` for the original native
comparison. `cargo xtask install-oracle` now reproduces this capture using Rust
orchestration and prints JSON without writing repository files. It accepts no
arguments and only executes the pinned helper functions with fixed inputs.

The explicitly invoked replay test
`cargo test -p xtask frozen_installer_platform_capture_is_reproducible -- --ignored`
passes against both preserved refs. It is ignored in ordinary tests because it
executes the source shell oracle; native fixture comparisons remain ordinary tests.

`platform_detection_matches_both_pinned_shell_oracles` passes all 20 cases.
Successful OS/architecture tokens match byte-for-byte. Error cases compare only
the unsupported-system/architecture category; Workdeck's native diagnostics and
non-npm installation guidance still differ. No full error-output parity or source
interval completion is claimed. The entire 20,132-byte baseline `install.sh`
ledger interval remains unmapped pending complete implementation and evidence.
