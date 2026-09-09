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

The release-directory README and marker file have separate ledger records and
are not covered by this archive.
