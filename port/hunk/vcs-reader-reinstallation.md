# Reinstalling an unchanged VCS reader

The production VCS capability installation path cleared presentation for every
unversioned reader, even when reinstalling the same runtime capability after an
identical reload. The new regression failed at the expected loaded-status check:
the installer had replaced loaded presentation with a new loading cycle.

`SourceLoaderBinding` now records the capability's existing private runtime ID.
A binding with matching semantic source identity retains its state when either
the provider attests the source or the exact runtime capability is unchanged.
A genuinely new unversioned capability still resets presentation and refreshes
an already-open gap. Actual document replacements still retire unattested
bindings before installation, as before.

The regression uses deferred VCS materialization and the production installer,
not the test-only reader attachment path. It checks pending-completion retention,
loaded rows and selection across identical reload/reinstallation, then installs
a fresh capability for an otherwise identical changeset and checks loading plus
fresh rendered source. Runtime IDs are not added to serialized source metadata;
this change cannot construct a reader or grant authority from serialized data.

This is native lifecycle correction and additional regression evidence. No Hunk
source interval is newly mapped and full provider/reload parity is not claimed.

Scoped validation: all 1,185 TUI library tests pass with zero failures, ignored
tests, or filters in 16.23 seconds; formatting and diff checks pass. Full
workspace verification, strict Clippy, and strict port audit were not rerun for
this change. The latest audit remains incomplete with 272 unmapped intervals
and 11 cached upstream commits.
