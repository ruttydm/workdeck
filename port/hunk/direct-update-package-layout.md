# Direct updater/package layout alignment

Inspection found that `release package` writes target-triple archive names and a
`workdeck-TARGET/` wrapper, while the direct updater requested short platform
names and expected a root-level executable. Neither matched the release workflow.

The direct updater now selects the same five targets as `.github/workflows/release.yml`:
`aarch64-apple-darwin`, `x86_64-apple-darwin`,
`aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`, and
`x86_64-pc-windows-msvc`. It carries the wrapped executable path in
`WORKDECK_DIRECT_BINARY`, used by both Unix and Windows replacement commands.
Archive checksum URLs retain the packager's adjacent `.sha256` naming.

This fixes a product integration defect, not the complete native updater port.
The direct-update implementation still delegates downloading, unpacking and
replacement to shell/PowerShell. The verified native staging implementation must
be integrated into the shipped binary, with signature validation and an
installation transaction, before that runtime boundary is complete. No source
ledger mapping is advanced by the layout correction.

All 63 `workdeck-cli` updater unit tests pass after the correction. The direct
macOS and Windows tests explicitly assert target-triple URLs and wrapped binary
paths. Formatting and whitespace checks pass. These are invocation-level tests,
not live release-download, extraction or binary-replacement evidence.
