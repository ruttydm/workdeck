# Install compatibility migration

The pinned Hunk install-VM tree used a Firecracker guest, Docker controller, npm/pnpm registries,
and Bun. Workdeck cannot ship those runtimes: release installation is an authenticated archive
transaction around one native `workdeck` executable. `cargo xtask install-vm` is the native Rust
replacement. It reads the pinned scenario manifest through `git show`, runs the equivalent
installer, archive, skill, updater, and authenticated session checks locally, and emits the same
bounded JSON/JUnit evidence shape.

The native suite retains the important safety boundaries: least-privilege execution, no shell
interpolation, immutable fixture identity, safe artifact paths, non-overlapping cleanup roots,
atomic runtime locks, checksum/signature verification, and daemon successor hand-off. The
Firecracker-only prerequisites (`/dev/kvm`, Docker, guest rootfs, Node and package managers) are
intentionally absent; their package-manager scenarios are represented by the native archive
matrix and are not silently skipped. There is no package manager runtime in the replacement.

`cargo xtask install-vm --list` lists all fifteen pinned scenarios. A selected run can write
`result.json` and `junit.xml` with `--scenario`, `--output`, and `--reuse-fixtures`; `--clean`
only removes a path below the repository-owned `tmp/install-vm` root. Source hashes, scenario
definitions, and pin validation are checked on every `cargo xtask verify` and port audit.
