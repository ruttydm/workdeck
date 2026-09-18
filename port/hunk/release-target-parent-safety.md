# Release target parent safety

Review of the existing release-plan writer before reuse for generated artifacts
found that leaf checks alone permitted parent-directory symlink redirection.
Release targets now require nonempty repository-relative normal components and
real, existing parent directories. Parent checks run before original-byte backup,
before each write, during post-write verification and before each rollback.
Rollback preserves a replaced/symlinked parent as a conflict instead of following
it outside the repository.

Tests cover traversal/absolute paths, file-valued parents and Unix directory
symlinks with external target bytes preserved. Existing transaction tests remain
the regression suite for successful application, interruption and rollback.
These checks reduce path-redirection risk but are not race-free directory-handle
operations; a hostile concurrent rename between checking and opening remains an
open hardening concern. No artifact write command is added by this change.

The three release-application unit tests and real CLI release-plan round trip
pass, as do strict xtask Clippy, formatting and diff whitespace checks. This is
Workdeck transaction hardening in preparation for artifact writes, not new Hunk
source-ledger coverage.
