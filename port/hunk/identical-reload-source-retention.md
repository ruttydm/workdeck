# Identical reloads retain live source state

The native reload commit reconciled source loaders before checking whether its
changeset actually changed. For an unversioned reader, that removed loaded text
and executable source ownership even on an identical reload, while the expansion
remained open. The new controller regression failed at its loaded-status assertion
before the fix and passed after reconciliation was restricted to changed
documents. Explicit full resets still retire all source state.

The pinned reducer returns its original state for the same document. The pinned
hook tests for unchanged soft reloads and changed-file invalidation both pass on
main and stable (two tests, twenty-two assertions per pin). The native regression
uses an explicit unversioned reader and checks loaded text, ownership, expansion,
selection, cursor, and rows across an identical reload. It then changes content,
checks source-identity retirement, and installs a fresh reader to verify that the
new expansion uses fresh rather than stale text.

This is an additional native runtime regression using the existing source-loader
fixture, not a blanket mapping of the source hook or its alpha-file test helpers.
No ledger coverage is added. Other semantic-document equivalences, provider
replacement routes, and full lifecycle parity remain separate requirements.

Scoped validation passes: all 1,184 TUI library tests with zero failures,
ignored tests, or filters in 12.72 seconds; formatting and diff checks pass.
The full verifier, strict Clippy, and strict port audit were not rerun for this
change. The latest audit remains incomplete, with 272 unmapped intervals and
11 cached upstream commits; no fresh upstream catch-up is claimed.
