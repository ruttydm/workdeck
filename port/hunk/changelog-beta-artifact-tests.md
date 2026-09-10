# Prerelease artifact test translation

The complete pinned-main `prereleases publish without becoming stable` group in
`scripts/generate-changelog.test.ts` is translated to five `source_beta_*` tests
in `xtask/src/changelog/website/artifacts.rs`.

Mapped interval: bytes `[27169,29637)`, lines 751–810, totaling 2,468 bytes.
The original SAMPLE, explicit dates, legacy date preservation, beta-only Markdown,
anchor strings and null latest metadata are retained. Tests call the production
artifact generator and inspect returned page/index/date/latest artifacts.

All 13 source expectations are preserved. The no-stable-install check uses
`cargo install` under Workdeck naming instead of the source npm installer, with
an additional assertion retaining the npm-absence rule. Update-command absence
uses `workdeck update`. Thus the Rust tests execute 14 assertions.

The source group passes five tests / 13 expectations in the disposable pinned
Bun runtime; all five translated Rust tests pass. MIT Modem Labs Inc. attribution
is retained in the implementation and notices. The adjacent orphan cleanup group
remains unmapped, as does the runtime generator.

Strict audit still fails with 1,257 files, 1,459 intervals, 281 unmapped intervals
and 92 queued upstream commits. The unchanged unmapped count reflects shortening
the existing interval by 2,468 bytes. Strict xtask Clippy, formatting and diff
whitespace checks pass.
