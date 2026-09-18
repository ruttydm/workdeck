# Changelog change-entry test translation

The complete `change entry parsing` describe block in pinned main's
`scripts/generate-changelog.test.ts`, bytes 2294–3299 (end exclusive), lines
102–131, is translated without changing its four inputs or expected objects.

The Rust tests in `xtask/src/changelog/website.rs` are:

- `entry_recovers_pull_request_from_changesets`
- `entry_handles_commit_link_without_pull_request`
- `entry_strips_legacy_bare_sha`
- `entry_keeps_plain_prose`

Each compares the complete serialized object, including omission of a missing
pull-request field. Running the source `change entry parsing` group under Bun
1.3.14 passed all four tests and four assertions on each pinned source tree.
`cargo test -p xtask changelog::website::tests::entry_` passed all four Rust tests.

Only this complete test block is mapped. The surrounding test-file intervals
remain unmapped, as does the incomplete runtime generator. Splitting the original
record increases the number of unmapped intervals from 277 to 278; it does not
increase the remaining byte coverage or represent a completed source file.
