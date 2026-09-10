# Bundled installer assets

Release packaging now includes the four current Workdeck skill sources under
`skills/<name>/SKILL.md` inside the platform archive wrapper:

- `workdeck-review`
- `workdeck-extensions`
- `workdeck-release`
- `workdeck-launch-video`

`xtask::release_entries` reads their exact repository bytes, requires every file
to exist and assigns mode 0644. Both tar.gz and ZIP writers receive these entries
alongside the executable, license inventory, SBOM and third-party notices. The
release pipeline still attaches provenance separately before writing an archive.

The `release_entries_retain_every_complete_syntax_notice` test checks skill entry
names, bytes and modes, then writes both archive formats and reads every entry
back to compare exact contents and entry counts. This is local packaging evidence,
not a signed release or platform installer smoke test.

Remaining integration: the native authenticated installer currently commits only
the executable. It must install the accompanying asset tree with appropriate
authentication and recovery. `workdeck skill path` now prefers an existing skill
under the executable directory or its ancestors, so source and unpacked native
archive layouts resolve without writing user configuration. If no skill exists,
the prior embedded-content fallback still materializes it under user configuration.
Metadata installation, asset-update rollback and full installer orchestration
remain open. This change does not map the baseline installer interval complete.

`xtask release package` now includes `metadata.json` with the four source fields
from `scripts/build-prebuilt-artifact.ts`: `packageName`, `os`, `cpu` and
`binaryName`. The package name matches the native `workdeck-<target>` wrapper;
OS tokens remain `darwin`/`linux`/`windows`, CPU tokens remain `arm64`/`x64`, and
the executable is `workdeck` or `workdeck.exe`. Output uses indented JSON with a
trailing newline. Unsupported target triples fail before packaging. The explicit
five-target metadata unit test passes, including unsupported-target rejection.
This does not establish all behavior of the original staging script: its source
ledger record remains unmapped.

The resolver translates the native-layout ancestor search from Hunk's
`src/core/run/paths.ts` and normalizes names by trimming whitespace and lowercasing.
It retains Workdeck's additional release and launch-video skills. npm-specific
layout candidates are not introduced into the native product. Unit coverage checks
binary and missing-directory roots, aliases, invalid names and no directory
creation. The CLI regression checks alias equality and that packaged/source lookup
leaves a temporary user config directory empty. These mappings remain partial;
the source paths module contains additional configuration and canonicalization
behavior not established by this resolver.

The CLI adapter now delegates to the existing `workdeck-core` name and ancestor
resolver rather than duplicating it. This also preserves the core's native
`workdeck/skills` and `share/workdeck/skills` layout candidates. A relocated-binary
integration test copies the executable outside the source checkout, runs it from
an unrelated empty directory, and checks that it returns the installation's
`share/workdeck/skills/workdeck-review/SKILL.md` sentinel unchanged. Both the
temporary user configuration and working directory remain empty. This test and
the two CLI-adapter unit tests pass on macOS; it does not establish native Windows
or Linux execution evidence.

## Archive authentication before asset installation

The release workflow attests complete archives after packaging, separately from
the embedded binary attestation. `install::prepare_authenticated_archive` now
uses that published archive attestation through the declared `gh` verifier,
enforcing the repository, release workflow, full source commit, tag ref, GitHub
OIDC issuer and predicate policy with a 120-second deadline. Missing or rejected
attestations fail rather than falling back to checksum-only trust.

Verification operates on a private archive snapshot after checksum validation
and before extraction. The open snapshot and its named path are rehashed after
verification; changed content is rejected. The temporary authentication directory
is removed on success or error. Existing checksum-only staging remains explicitly
separate and is not evidence of publisher authentication.

Both staging tests pass on macOS, including injected verifier success, rejection,
snapshot modification and cleanup. The injected callbacks test orchestration,
not real signatures or GitHub availability. This new API is not yet wired into
the complete asset installation transaction; the existing updater continues to
authenticate its binary separately. No signed release was fetched or published.

## Integration checkpoint at `04ffe9bd`

- Strict `cargo clippy -p workdeck-cli -p xtask --all-targets -- -D warnings`
  passes with incremental compilation disabled and two build jobs.
- `cargo xtask architecture check` passes: 12 production crates, one shipped
  executable, zero dependency or source-reachability violations.
- Strict `cargo xtask port audit` fails: 1,257 baseline files, 1,459 intervals,
  280 unmapped records and 92 cached upstream delta commits. Five stable-only
  commits are tracked; this count does not prove their implementation complete.

No upstream fetch was performed at this checkpoint. These checks validate the
current installer additions' compilation and ownership boundaries, not whole-
product parity, performance, native platform coverage or signed release delivery.
The next installer integration remains the authenticated asset-tree transaction
with recovery coordinated with binary installation.

## Recoverable skills-tree transaction

`install::install_skill_tree` now copies a caller-authenticated source tree into
a private temporary directory on the destination filesystem. It requires all
four bundled `SKILL.md` files and bounds copying to 10,000 entries, 64 MiB and
64 levels. Links, special files, Windows reparse points and source/staging
overlap are rejected. Files are synchronized before publication.

The transaction uses the persistent native installer lock. It creates a new
recovery directory, moves an existing skills tree into `recovery/skills`, then
publishes the prepared tree using exclusive rename (NOREPLACE on macOS/Linux).
An injected publication failure restores the previous tree; restoration will
not overwrite a competing destination, leaving the recovery tree available
instead. Existing recovery directories are never reused or overwritten.

This low-level API requires an authenticated, quiescent source and coordinated
destination parents. It does not itself authenticate assets, commit a binary,
install metadata, guarantee crash-durable directory changes or provide an atomic
multi-resource installation. It leaves recovery directories and the lock file
intentionally. Its native transaction test passes on macOS for successful
replacement, retained old contents, injected rollback and recovery collisions.
Windows and Linux execution remain unverified. No ledger interval is completed.
