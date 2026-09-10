# Native verified archive staging

The implementation now belongs to `workdeck-cli::install`; xtask delegates its
installer commands to that library. `prepare_verified_archive` returns an owned
temporary-directory handle without printing or installing, for integration with
the native updater. All 27 installer unit tests moved with the implementation
and pass in `workdeck-cli`; they are no longer xtask unit tests. Existing xtask
suite counts below describe the earlier verification run, before this move.
The dependency graph still ships only the `workdeck` product executable.
After relocation, all 27 library installer tests and the real xtask staging CLI
test pass; formatting and whitespace checks pass. The old xtask implementation
is replaced by four re-exports, with no duplicate installer implementation.

`cargo xtask install-stage ARCHIVE CHECKSUM_FILE` verifies and extracts a local
release package into a newly allocated temporary directory, returning its path
in JSON. Success retains that directory for the next installation step; callers
own its eventual cleanup. Failures drop the staging directory automatically.
It never replaces an installed executable or edits shell profiles.

The command snapshots the bounded input into a private temporary file before
hashing. Package inspection and extraction consume that same snapshot, preventing
an archive-path replacement between those operations from changing extracted
bytes. Existing inspection enforces archive limits, paths, duplicate detection,
regular-file types and required package members. ZIP and gzip tar are supported.
Extraction uses newly created files, without preserving archive ownership or
special permissions. On Unix, `workdeck` receives mode 0755 and other files 0644.

The checksum manifest must already be trusted by the caller. This is integrity
verification, not publisher authentication: the JSON explicitly reports
`signatureVerified: false` and `installed: false`. Release resolution, signature
verification, installation replacement/rollback and profile updates remain open.
No source-ledger interval is marked complete for this incremental implementation.

The ZIP staging regression verifies extracted executable bytes, six required
package members, executable permissions on Unix, checksum rejection and unchanged
input-directory contents. The gzip-tar staging regression verifies binary payload
preservation, executable and content modes with setuid/setgid removed, rejection
of links and missing licenses even with a matching checksum, cleanup when the
staging handle is dropped, and unchanged input archives. All 27 installer unit
tests pass. Existing archive-inspection tests cover malformed tar/ZIP and path
hazards. The real CLI test now checks retained staging files, the integrity-only
JSON report, empty stdout on checksum/argument failures, unchanged archive bytes
and no repository-state creation. It removes the exact staging directory it
created after validating its location and contents. Cross-platform execution and
installer integration remain additional validation work.

## Local regression verification

After the staging CLI test was added, `cargo test -p xtask --all-targets` passed
394 tests with one existing ignored oracle-capture test: 362 unit tests, 22
changelog CLI tests, seven extension-catalog CLI tests, one installer CLI test,
one workspace-command CLI test and one terminal theme-probe test. Strict xtask
Clippy also passed. This is xtask-local evidence, not full workspace or native
release-matrix qualification.
