# Test-suite scheduling migration

The pinned Hunk `scripts/run-test-suite.test.ts` contract is represented by
`xtask::test_sharding`. Both pinned blobs are 2,555 bytes / 79 lines with
SHA-256 `7db067c683d0f16daf940f21b9bee91c1d47676f8d38c5bfce326b09e81243d9`.

Workdeck runs the complete locked Cargo workspace test command. Its native
policy retains Hunk's bounded Linux override semantics under
`WORKDECK_TEST_SHARDS`: Linux defaults to at most two build jobs, accepts a
positive value from 1 through 64, and rejects malformed or excessive values;
non-Linux hosts remain serial. Cargo's test harness does not expose Bun's
file-level `--shard` switch, so a multi-shard invocation pins test threads to
one and records the shard identity for CI orchestration rather than silently
dropping tests. Process termination is best-effort and tolerates an already
stopped child.

`cargo xtask test` is still the authoritative full-workspace entry point and
keeps the isolated Git environment. The verifier reads both protected source
blobs through `git show`, checks every source test description and error
surface, and runs native policy, command-shape, and termination tests. No Bun
runtime, JavaScript test mirror, or npm package is retained.

The pinned `scripts/run-test-suite.ts` launcher is covered as well (5,558 bytes
 / 167 lines, SHA-256
`3fbee1810fbe770035651aa88372f2ed9fa7ca34096009191cb086dea113d598`). Its
native replacement is the `cargo xtask test` command and its Cargo-owned
workspace runner; no package-manager process is spawned.
