---
name: workdeck-release
description: Validate and package a Workdeck release with semantic-port, license, SBOM, and provenance gates.
---

# Workdeck release

Run `cargo xtask port audit` first; a release is forbidden while any Hunk ledger record is
unmapped. Then run `cargo xtask verify`, dependency security/license policy, and native smoke
tests. Package a built target with `cargo xtask release package --target TRIPLE`.

Every archive must contain the sole `workdeck` executable, LICENSE, THIRD_PARTY_NOTICES,
licenses.json, and sbom.cdx.json. GitHub release CI produces checksums and signed provenance.
