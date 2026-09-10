# Shared release verification policy

`workdeck_cli::install::attestation` now owns the release-verifier argument policy
and bounded child-process lifecycle previously local to xtask. Packaging imports
those same functions. The policy requires an explicit GitHub repository, full
source commit digest, tag ref, release workflow identity, GitHub Actions OIDC
issuer and SLSA provenance type, and rejects self-hosted builders. Child failure
and deadline expiry remain failures; timed-out children are killed and reaped.

All eight existing provenance tests pass after the move, including policy
validation and real child-process success/failure/timeout probes. No duplicated
policy was introduced and no source-ledger record changed. This is shared native
policy code, not a completed updater authentication path: packaging still invokes
the declared `gh attestation verify` program, and updater integration must verify
the exact staged bytes before replacement. Structural bundle decoding and checksum
matches alone must never be reported as publisher authentication.

`authenticate_binary` now accepts owned binary bytes, a bundle and an explicit
release identity. It creates a private snapshot, invokes the declared `gh`
verifier with the shared policy and deadline, checks that snapshot contents remain
unchanged, and returns the original owned bytes only on success. This supplies
the updater with the same payload that was presented for verification, without
rereading an external archive or staging path afterward. Snapshot files are
removed on success or failure; no installation target is touched.

Two native tests inject verifier callbacks to check exact binary/bundle inputs,
returned bytes, rejection of failures and snapshot mutation, and cleanup. These
are control-flow and snapshot tests, not real Sigstore authentication evidence.
Real signed-release verification and updater integration remain open.

## Composed local archive operation

`install_authenticated_archive` now composes checksum-verified staging, bounded
binary/bundle reads, attestation of owned snapshot bytes, and backup-preserving
native binary replacement. Its caller supplies trusted repository/commit/tag
identity separately from archive metadata. The staged package must include
`provenance.sigstore.json`. Authentication completes before the target-side lock,
backup or replacement is created. Accompanying package assets are not installed
by this operation; release downloading and the public updater connection remain
open, as do the replacement primitive's documented concurrency/platform limits.

A temporary ZIP integration test verifies that an injected authentication failure
leaves the old binary untouched with no destination lock or backup. Injected
success installs the verified payload and preserves exact original backup bytes.
This exercises real staging and file replacement, but the verifier is a test
callback: it is not evidence of a real signed release passing authentication.
