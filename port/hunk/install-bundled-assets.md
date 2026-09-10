# Bundled installer assets

## Latest regression checkpoint (`9e19a586`)

Strict Clippy passes for `workdeck-cli` and `xtask` with `--all-targets -- -D
warnings`. All 49 tests in `workdeck-cli/tests/cli.rs` and both tests in
`xtask/tests/install_cli.rs` pass on macOS. These cover the current standalone
installer routing alongside existing issue, agent, reference, import/export,
session, extension, update and read-only CLI regressions. They are not a complete
workspace, native-platform matrix, benchmark or signed-release verification.
No source ledger records were changed during this checkpoint.

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

`install::install_authenticated_skills` connects complete-archive attestation and
private staging to the recoverable skills-tree transaction. It requires exactly
one staged package root and installs only that root's `skills` directory. Staging
and authentication must finish before any destination temporary files, lock or
recovery directory are created. The authenticated staging handle remains owned
until installation completes and is then cleaned up.

An integration test builds an actual ZIP with all four skill payloads and the
required package entries, exercises checksum validation, staging and installation,
and verifies exact skill bytes. An injected archive-verifier rejection leaves an
empty destination completely untouched. Both asset tests pass on macOS. The test
uses injected attestation results, not real signing evidence. Binary, metadata,
PATH edits and skills still need one coordinated installer workflow; this API
alone is not full installer completion.

Prebuilt metadata now has one shared Rust schema in the installer library, used
by release packaging as well as installation. Decoding is limited to 64 KiB,
rejects duplicate/unknown fields and requires all four fields to match the known
native target. Authenticated skills installation validates metadata against its
`workdeck-<target>` wrapper before destination writes. A malformed-metadata
integration case leaves the destination empty. The shared metadata test, existing
five-target packaging test and both asset tests pass on macOS. Metadata is still
not committed to the installation directory.

Authenticated skills installation now requires a caller-selected native target
independent of archive metadata. Unsupported selections fail before staging or
verification; the authenticated archive wrapper must match that selection before
any destination writes. An integration regression passes a valid macOS arm64
archive with a Windows x64 selection and confirms rejection with an untouched
destination. It also checks that unsupported selections never invoke staging.
Both asset tests pass with these cases. The eventual installer composition root
must derive this selection from its platform detection; the asset API deliberately
does not infer the desired target from the archive itself.

## Metadata file transaction

`install::metadata::install` validates caller-authenticated bytes against the
selected target before writes. It permits only a `metadata.json` destination,
uses the native installer lock and creates a new recovery JSON record with the
absolute destination and exact original bytes (null when absent). It never
overwrites a recovery record. Existing metadata reads are bounded to 64 KiB and
reject links/reparse points using the shared opened-handle reader.

The replacement is synchronized in a destination-directory temporary file;
existing permissions are preserved and content/permissions are checked again
before publication. An absent destination uses no-overwrite publication. New
files inherit the temporary file's private Unix mode. Recovery is retained on
later failure. Parent races and concurrent writes after the last check still
require caller coordination; automatic rollback across other installer resources
and directory crash durability are not provided by this primitive.

All 56 installer library tests pass on macOS after this change. The new test
covers invalid input without writes, first creation, exact non-UTF-8 original
recovery, recovery collisions and preservation of an injected concurrent edit.
The metadata transaction has not yet been wired into the coordinated installer.

## Complete first-install publication

`install::create_authenticated_installation` now coordinates a new native
installation root. It authenticates the complete archive, validates its wrapper
and metadata against an independently selected target, requires all four skills,
and copies the package into a private sibling directory. The binary is relocated
to `bin/workdeck` (or `bin/workdeck.exe`), while skills, metadata, license files,
SBOM, provenance and remaining packaged content stay alongside `bin`.

The prepared tree is published by a single exclusive directory rename. Existing
roots, including empty directories created just before publication, are never
replaced. Failed staging/preparation cleans up the private tree. The copy has
bounded entry/depth limits and a 2-GiB overall budget; opened files cannot grow
beyond the observed size during copying. On Unix the executable receives mode
0755. The parent must already exist and remain coordinated by the caller.

The native first-install test checks full layout publication, authentication
rejection, injected prepublication failure and existing/racing destinations.
It injects staging and does not establish live archive-signature verification.
This API does not update an existing installation, edit PATH, resolve/download a
release, implement CLI orchestration or guarantee directory crash durability.
Those remain required parts of the original goal; no ledger interval is closed.

The explicit tooling entry point is now:

```text
cargo xtask install-create ARCHIVE CHECKSUM_FILE DESTINATION TARGET COMMIT TAG_REF
```

It authenticates against the fixed `ruttydm/workdeck` publisher and the supplied
full source commit/tag reference, then invokes complete first-install publication.
The destination root must not exist, and its parent must exist. Success emits
JSON containing the installed root, selected target, source identity, verified
archive-signature status and `pathModified: false`. Errors emit no success JSON.
The command does not infer trust from archive metadata, resolve the release
identity for the caller or silently update an existing root.

Both `xtask/tests/install_cli.rs` tests pass, covering the existing staging
command plus first-install argument, platform and identity rejection with an
untouched directory. No live signed-release installation has been claimed or
performed. Standalone executable/installer-script orchestration and automatic
release resolution remain outstanding beyond this Rust tooling entry point.

`install::install_release` now composes version parsing, native target selection,
independent GitHub tag-to-commit resolution, bounded HTTPS downloads, complete
archive authentication and first-install publication. It validates the version,
platform/architecture and destination before invoking the network path. An
existing destination is rejected, not updated. Downloaded temporary artifacts
remain owned through publication and are removed when the operation returns.

The first-install tests pass for version normalization and selected-target
propagation, pre-network existing-root/invalid-version rejection, and complete
publication behavior. Network/public-signature success remains untested against
a real release. This composition accepts a caller-selected platform/architecture;
automatic host/Rosetta selection, latest-version resolution, PATH changes and
standalone CLI integration are still outstanding.

`install::install_release_on_host` now supplies automatic platform/architecture
selection to that flow. Installer preflight and first installation share the
source-tested platform mapping. macOS obtains `sysctl.proc_translated` through
the native `sysctlbyname` interface, eliminating the previous external command;
missing/failed probes fall back to untranslated behavior as in the source.
Rosetta-translated x86_64 maps to arm64 on Darwin only. Unsupported host platforms
remain errors rather than being treated as Linux.

The 58 installer tests pass after sharing this detector. The first-install test
also exercises host selection with an invalid version, confirming no destination
writes. The existing dual-pin platform oracle covers translation-flag mapping,
but this host run does not prove execution under Rosetta itself or on other OSes.

## Standalone command

`workdeck install [VERSION] [--destination DIRECTORY]` exposes the host-selected,
signed-release first-install flow in the sole shipped executable. It is a global
headless command: no repository configuration is required. Version selection uses
the positional argument, then nonempty `WORKDECK_VERSION`, then the bounded GitHub
latest-release lookup shared with the updater. The resulting tag is independently
resolved to a commit before archive authentication. The destination defaults to `$HOME/.workdeck`, falling back to
`$USERPROFILE/.workdeck` when HOME is empty or absent. An explicit destination
overrides that selection. The parent directory must already exist. It leaves
PATH untouched and refuses existing roots. The `install` name is reserved from
extension CLI registration and recognized by builtin argument routing.

CLI coverage checks help and invalid-version rejection outside Git without
creating state, and rejects an existing destination before network access. This
does not prove installation from a live signed release. Automatic update-in-place,
conflict discovery, PATH flags and update-in-place semantics
from the original installer remain to be integrated; this explicit first-install
command is not a declaration of full installer parity.

The standalone command now gates release lookup on existing native conflict
observations from PATH and inactive Mise installation directories. Executable
conflicts require `-f`/`--force` or `WORKDECK_ALLOW_CONFLICTING_INSTALLS=1`;
unknown access remains an error even with force. Force permits coexistence only,
not replacement of the destination or deletion of another installation. The
gate lists observed conflicting paths. Its test verifies refusal, explicit
override and unchanged candidate bytes/no destination creation.

Complete source manager/version/removal diagnostics are
still missing. Lower-level archive/tooling installation APIs remain explicit
operations and do not implicitly run process-environment conflict discovery.

Inactive discovery now also inspects `.nvm/versions/node/*/bin/workdeck` (or
`workdeck.exe`) before Mise candidates, preserving sorted source glob order.
This is read-only legacy layout detection: no Node/npm process or runtime is
used. Missing roots yield no candidates; other directory-read failures propagate.
Both preflight and the standalone conflict gate share the combined scan. A native
test passes for absent roots, sorted inactive versions, nvm-before-Mise ordering,
force decisions with empty PATH and unchanged candidate bytes.

Conflict diagnostics now include the manager-shaped diagnostic alias, explicitly
labeled inferred ownership, and all three source PATH-order states: shadows the
target, is shadowed by the target, or is not on the current PATH. Legacy npm/Bun/
pnpm layout hints are recognized without executing those runtimes. nvm ownership
requires the `.nvm/versions/node` segment sequence. Unknown ownership stays
`another package manager`; no package ownership or version is fabricated.
All 62 installer tests passed before the final nvm matcher tightening, and the
focused conflict-description test passes afterward. Running-version probes and
complete package-specific removal diagnostics remain outstanding.

All 48 CLI integration tests passed before adding the default-root selection.
The updated installer CLI regression also passes: invalid versions create no
default root, and an existing default `.workdeck` is rejected before networking
without changing its contents. This default does not reinterpret the source
`WORKDECK_INSTALL_DIR` custom-bin environment variable, whose distinct payload
layout remains an outstanding integration requirement.
