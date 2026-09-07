# Hunk semantic-port ledger

Workdeck ports the pinned Hunk source tree without merging Hunk's unrelated history into the
Workdeck mainline. The source anchors are:

- `hunk-port/main-2c00f435` (`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`)
- `hunk-port/stable-v0.20.1` (`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`)

The `hunk-upstream` remote fetches branches and tags into namespaced refs. The tracked ledger is
generated from Git blobs, not from a vendored TypeScript source tree:

```console
cargo xtask port inventory
cargo xtask port status
cargo xtask port audit --allow-incomplete
cargo xtask port map --path PATH --disposition rust-reimplementation \
  --destination crates/... --evidence crates/...
```

`audit` without `--allow-incomplete` is the release gate. Every baseline byte must be covered by
exactly one ledger interval. A mapped interval must name both repository destinations and test or
verification evidence. Valid dispositions are Rust reimplementation, translated test, migrated
content, retained asset, Rust-generated replacement, and retained license.

Port commits name their records with `Hunk-Port:` trailers. Stable and catch-up commits additionally
carry `Hunk-Upstream:` trailers. An unmapped record is visible work, never an implicit waiver.

The five commits unique to Hunk `v0.20.1` are tracked separately in
`port/hunk/stable-fixes.jsonl`; the four functional regressions have Rust implementations and
named tests. They do not falsely mark the larger baseline blobs containing those files as ported.

## Terminal lifecycle work in progress

`oracles/pty-lifecycle.json` records the five passing baseline oracle cases and the test file's
absence from the stable pin. Native CLI subprocess tests now cover private PTY closure, macOS
controlling-terminal revocation, and the three shutdown signals with terminal streams. A dropped
terminal returns success after session retirement, while non-terminal-I/O errors remain failures.
The test harness owns and reaps its children and closes inherited PTY handles explicitly.

This is **not** complete lifecycle parity: the source's signal cases use pipes with brokering
enabled. Workdeck still rejects redirected interactive review output. The lifecycle source remains
unmapped until that behavior and its broker cleanup have genuine executable coverage.

Run the current native coverage with `cargo test -p workdeck-cli --test terminal_lifecycle`.
