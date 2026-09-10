# Native binary replacement primitive

`workdeck_cli::install::replace_binary_with_backup` accepts a target binary,
already-authenticated replacement bytes and a new backup-file path. It requires
an existing regular non-symlink `workdeck` or `workdeck.exe` destination. It stages
the new bytes on the destination filesystem, flushes them, writes and flushes an
exact original backup without overwriting an existing backup, rechecks the
original bytes/permissions, and atomically persists the replacement. Unix
replacement mode is 0755; backup permissions preserve the original. Temporary
replacement files are cleaned up on errors; completed backups remain recoverable.

This is a library primitive, not an enabled installer or updater transaction.
It neither authenticates payloads nor installs accompanying assets. Parent
directories are resolved once; callers must prevent concurrent parent replacement
and other writers racing the final rename. Observed precommit changes abort, but
the check is not a lock or a race-free compare-and-swap. Directory crash durability,
multi-file rollback, initial installation, signature verification and Windows
running-process replacement still require integration and tests.

All 30 installer tests pass, including successful replacement, exact retained
backup bytes, refusal to overwrite a backup, preservation of a precommit external
edit, temporary-file cleanup, and symlink-target rejection. Tests only mutate
temporary fixtures. No user-installed executable has been replaced. No source
ledger interval is advanced by this incremental native implementation.

Binary reads now validate the opened file handle and read at most the observed
length plus one byte. Growth, truncation and oversized declarations fail rather
than bypassing the bound. Unix opens also use no-follow and nonblocking flags
to reject symlink substitution and avoid FIFO blocking. This does not close the
final rename race, and equivalent Windows reparse-point handling remains open.
All 31 installer tests pass, including a bounded-read test with an endless input.
The broader `cargo test -p workdeck-cli --lib` run also passes all 271 tests.
This does not include CLI integration targets or the full workspace suite.

Cooperating replacement calls now hold an exclusive advisory lock on the
persistent destination-side `.workdeck-install.lock` file. The file is not
unlinked after unlocking, avoiding separate lock inodes for overlapping callers.
Content is never truncated, and Unix lock creation is mode 0600 with no-follow
and nonblocking opens. Contending calls fail before creating their backup or
changing the binary; a subsequent call succeeds after the first handle closes.
Tests also reject a Unix symlink lock without modifying its target. This only
coordinates callers of this helper: unrelated writers, hostile directory changes
and Windows reparse-point substitution remain outside the guarantee. The lock
file is intentional persistent installation state, not repository state created
by a read-only command.
