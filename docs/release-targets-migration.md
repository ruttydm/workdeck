# Release target selection migration

The pinned Hunk `scripts/build-bin.test.ts` contract is represented by
`xtask::release_targets::compile_target_for_host`. Both the baseline and
stable-v0.20.1 source blobs are 1,172 bytes / 25 lines with SHA-256
`0a198ec71b6a06c18a87ec49373222a4f37a356e1e053bc7afa979edf3bb19cb`.

Hunk selected Bun-specific x64 baseline runtimes because its default x64
runtime required newer CPU instructions. Workdeck compiles the equivalent
explicit Rust targets instead: `x86_64-apple-darwin`,
`x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, and
`x86_64-pc-windows-msvc`. Arm64 hosts return `None`, leaving the native Rust
target authoritative, and unsupported platforms remain unselected. The
release workflow's five-target matrix is the consumer of these native target
identities; no Bun runtime or npm package is generated.
Arm64 keeps the native Rust target selected by the host toolchain rather than a
runtime-specific compatibility bundle.

The verifier reads both protected source blobs through `git show`, checks the
complete test-description and expected-result surface, and runs native tests
for x64 libc selection, arm64 defaults, and unsupported hosts before the
ledger interval is mapped.

The pinned `scripts/build-bin.ts` launcher (2,863 bytes / 87 lines,
SHA-256 `46888364765826de991e42b1552eba9842760a10bba077d27f22c083a429bc40`)
is replaced by `cargo xtask release build`. It builds the single `workdeck`
Cargo binary for the selected target and copies it to `dist/workdeck`; no
alternate executable or package-manager runtime is produced.
