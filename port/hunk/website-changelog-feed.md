# Native release feed (partial generator port)

`cargo xtask changelog feed <markdown-file> <dates.json> [notes.json]`
prints RSS to stdout without writing repository state. Notes use the existing
minor-series keyed objects with an optional `summary` field. Branding and links
target Workdeck; release selection follows the pinned main generator.

The Rust implementation in `xtask/src/changelog/website/feed.rs` retains separate
prerelease items, anchors a stable promotion only when its own prerelease was
published, sorts same-day entries by version, and escapes XML summaries. An older
beta does not cause an unrelated current patch to receive a new versioned GUID.

`website-changelog-feed-oracle.json` freezes ten source-runtime outputs: eight
main cases and two stable cases. Stable-only fixtures deliberately cover stable
publication, not the main pin's changed prerelease semantics. The unit test
compares exact Hunk output. The CLI integration test compares the eight main
outputs with only product and origin substitutions, and checks missing arguments,
missing files, invalid JSON shapes, unchanged inputs, and absent repository state.

This does not complete `scripts/generate-changelog.ts`: no runtime ledger interval
is marked mapped by this work. Complete input-domain parity, site pipeline
integration and release publication verification remain open. The source is MIT-licensed by
Modem Labs Inc.; see `THIRD_PARTY_NOTICES` and the source-level attribution.

## Initial implementation verification (`0c5dba98`)

- Exact frozen-feed unit test: ten cases passed.
- `cargo test -p xtask --test changelog_cli`: all 13 tests passed.
- `cargo test -p xtask --all-targets`: 339 tests passed, one existing oracle-capture
  test ignored (317 unit, 13 changelog CLI, seven catalog CLI, one command and one
  PTY test passed).
- `cargo clippy -p xtask --all-targets -- -D warnings`: passed after moving the
  test module below the production items.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Pinned main source `feed` test group: seven tests / 14 assertions passed in the
  disposable Bun oracle. This is source behavior evidence, not a claim that all
  seven original tests were translated in that initial commit. The subsequent
  [feed-test translation](changelog-feed-tests.md) now maps their complete block
  to seven genuine Rust tests with the original 14 assertions.
- Strict native `xtask port audit`: failed as expected with 1,257 baseline files,
  1,452 intervals, 280 unmapped intervals and 92 queued upstream commits. The
  upstream count reflects the existing fetched refs, not a new remote fetch.

## Calendar normalization continuation

The original Chrono-based feed date formatter differed from both source pins:
February 30 normalizes into March in Hunk, year-only and year-month ISO inputs
default missing fields, and signed years extend beyond Chrono's supported range.
The replacement uses an ASCII ISO parser and proleptic Gregorian day arithmetic,
including the source's inclusive 100,000,000-day epoch limit. Day values 1–31
normalize across short months; zero and values above 31 remain invalid. Negative
zero years, surrounding whitespace, embedded timestamps and malformed Unicode
inputs are rejected, matching the captured source behavior.

`website-changelog-feed-date-oracle.json` records 38 date cases from each pinned
generator under Bun 1.3.14. The Rust test compares all 76 values both directly and
through production RSS output. Cases include century leap rules, year zero,
signed years, both range boundaries and adjacent invalid days. This fixes observed
date mismatches; it is not a blanket mapping of the runtime generator.

Validation: `cargo test -p xtask feed` passes nine unit tests and the feed CLI
integration test. Strict xtask Clippy, workspace formatting and diff whitespace
checks pass. The ledger is unchanged by this correction.
