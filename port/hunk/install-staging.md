# Native verified archive staging

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
hazards. Real CLI staging, cross-platform execution and installer integration
remain additional validation work.
