# Native prebuilt-install smoke migration

`cargo xtask install-smoke` replaces the package-manager `scripts/smoke-prebuilt-install.ts`
workload. It stages the host-specific Workdeck artifact, validates the release metadata and
bundled skills, copies the executable into an isolated install root, and executes the real binary
for `--help`, `--version`, and both bundled skill lookups. The test uses a temporary HOME and
canonical PATH, so neither repository state nor user configuration is written.

The native contract checks the one `workdeck` executable, license/notice/SBOM/provenance payloads,
skill files, Workdeck naming, and the absence of package-manager/runtime dependency paths. The
source is read from both protected pins: baseline 8,882 bytes / 248 lines and stable 7,997 bytes /
222 lines, with their SHA-256 values recorded in `xtask/src/install_smoke.rs`.
