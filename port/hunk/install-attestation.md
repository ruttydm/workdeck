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

## Independent release identity lookup

`resolve_release_identity` now resolves the exact requested version's tag through
the fixed Workdeck GitHub Git API. It accepts lightweight commit refs and peels
at most eight annotated tags, verifying each returned object identity. Wrong refs,
invalid SHA-1 object digests, non-commit endpoints and cycles fail. Requests require
HTTPS, disallow redirects, cap each JSON response at 1 MiB and share a 30-second
deadline. The resulting commit and tag are independent of package metadata and
can constrain attestation verification. Lookup alone is not authentication.

Two injected-response tests cover lightweight/annotated tags and rejection cases.
Live GitHub API behavior and complete updater orchestration remain unverified.

## Direct updater connection

macOS/Linux direct-update invocations now carry a typed `NativeDirectUpdate`.
The production runner resolves the expected commit, downloads the pinned archive
and checksum, authenticates the staged binary with the declared `gh` verifier,
and replaces the existing binary with a timestamped retained backup. Progress
labels this as a native signed GitHub update. Failure returns a nonzero update
result and never falls back to command execution. Other package-manager channels
are unchanged. Windows retains its previous process-exit handoff pending a native
implementation; obsolete Unix command construction is retained but bypassed until
replacement qualification permits its removal.

The 281-test CLI library suite passes after connection. Platform-matrix tests
assert native selection and exact requested version/target. A further production
runner test uses an invalid version to prove failure precedes network or file
writes and does not execute a fallback command. No live update was run. Live
signed-release verification, cross-platform qualification, asset installation,
crash recovery and the documented filesystem race limits remain release gates.

## Live release availability

During follow-up verification of `666a6ffe`, the read-only command
`gh release list --repo ruttydm/workdeck --limit 5 --json tagName,isDraft,isPrerelease`
returned `[]`. No release was created or published. A real Workdeck signed-release
update cannot be demonstrated against that currently empty release listing;
local failure/snapshot tests are not a substitute. This does not block other
port work or authorize publishing a release merely to satisfy the test.

The subsequent `cargo test -p workdeck-cli --all-targets` run completed successfully
on the unchanged `666a6ffe` implementation. It includes 282 library tests, 46 CLI
integration tests, 12 Git integration tests, 27 review-conformance tests, nine
terminal-lifecycle tests and 98 terminal-pager tests. These native macOS results
cover the current command/TUI regression suite, not full Hunk source parity or
the unexecuted platform release matrix.
