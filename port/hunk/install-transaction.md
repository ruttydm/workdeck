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
