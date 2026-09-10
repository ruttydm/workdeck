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
