# Changelog feed test translation

Source: MIT-licensed Modem Labs Inc. Hunk baseline
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`,
`scripts/generate-changelog.test.ts`, bytes `[14502,17853)`, lines 406–502.
The complete `describe("feed")` block is translated to the seven `source_feed_*`
tests in `xtask/src/changelog/website/feed.rs`.

| Source test | Rust test suffix | Assertions |
| --- | --- | --- |
| emits one dated item per published series | emits_one_dated_item_per_published_series | 3 |
| publishes an anchored item for each dated prerelease | publishes_anchored_dated_prerelease | 3 |
| gives a later stable release a distinct series GUID | stable_promotion_has_distinct_guid | 2 |
| does not re-announce a stable patch because an older version had a beta | older_beta_does_not_reannounce_current_patch | 2 |
| notifies a stable patch after a beta in an established series | patch_beta_promotion_gets_versioned_guid | 2 |
| orders same-day releases by semantic version | same_day_series_follow_semantic_version_order | 1 |
| escapes XML in summaries | escapes_xml_in_summaries | 1 |

Inputs retain the original dates, Markdown and summaries. The shared source
SAMPLE is read from `website-changelog-test-sample.md`; its declaration remains
accounted for separately in the still-unmapped source preamble. The helper calls
the production Rust parser, grouping and RSS renderer. Test-only Hunk branding
keeps the original expected strings unchanged; the public CLI uses Workdeck.
Item counting translates `split(...).length - 1` to non-overlapping match counting.
The semantic-version ordering assertion retains the source regular expression.

`cargo test -p xtask source_feed_` passes all seven tests / 14 assertions. The
pinned main source group also passed all seven tests / 14 assertions in the
disposable Bun oracle during the preceding feed implementation. Frozen full-feed
comparisons remain additional evidence, not replacements for these source tests.

Only this test interval is mapped. Generator runtime parity, malformed-date
behavior, artifact generation and surrounding source tests remain incomplete.

Strict audit still fails with 1,257 baseline files, 1,454 intervals, 281 unmapped
intervals and 92 queued upstream commits. Unmapped interval count increases by one
because the completed middle block splits one incomplete interval into two;
unmapped source bytes decrease by exactly 3,351. Formatting, diff whitespace and
strict xtask Clippy checks pass after the translation.
