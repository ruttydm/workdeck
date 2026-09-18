# Historical Hunk release measurements

`port/hunk/historical-release-benchmarks.json` retains all 22 release-report
JSON files under `benchmarks/release/bench-*.json` at Hunk baseline
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. These MIT-licensed Modem Labs
measurements describe Hunk versions, not Workdeck performance or release acceptance.

Regenerate or verify with Rust tooling:

```console
cargo xtask benchmark historical-release
cargo xtask benchmark historical-release --check
```

Each entry records its original path, Git blob, byte count and exact UTF-8 source
text. JSON escaping in the enclosing archive is reversible: decoding `sourceText`
recovers every original byte, including whitespace and line endings. The generator
reads preserved Git objects, checks byte counts and JSON validity, and requires
exactly 22 reports. The regression regenerates the entire archive and compares
its bytes. It neither executes historical tooling nor treats historical thresholds
as Workdeck's stricter performance gate. Hunk attribution is retained in the archive
and `THIRD_PARTY_NOTICES`.

`cargo xtask verify` and the pinned `cargo xtask port audit` run the same
read-only archive check. A disposable shared Git checkout tests both missing
archive data and modified report contents, verifies rejection, and confirms
that checking never repairs the input. Custom-baseline inventory audits do not
require this Hunk-specific archive.

The release-directory README and marker file have separate ledger records and
are not covered by this archive.

The README's guidance is migrated separately in
[release benchmark snapshots](release-benchmark-snapshots.md), including the
distinction between historical comparator thresholds and strict port acceptance.
