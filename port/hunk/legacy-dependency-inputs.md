# Legacy dependency input migration

The pinned Hunk tree contained six JavaScript package manifests and lockfiles:

- `bun.lock`
- `website/bun.lock`
- `test/cli/install-vm/controller-deps/package-lock.json`
- `package.json`
- `website/package.json`
- `test/cli/install-vm/controller-deps/package.json`

Workdeck is a single Rust executable and does not retain or execute Bun, Node,
React, OpenTUI, npm, or a JavaScript engine. Their complete pinned bytes are
represented by the generated `legacy-dependency-inventory.json`. The inventory
records every source hash, byte count, line count, and parsed package-entry
count; `xtask` reads the pinned blobs through `git show` and rejects any drift.
Cargo manifests and `Cargo.lock` are the only active dependency authority.
The verifier and its unit tests are the executable parity evidence for this
replacement, and the original files remain available from the protected
`upstream/hunk/*` refs.
