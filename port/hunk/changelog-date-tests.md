# Original date-resolution test translation

Pinned main `scripts/generate-changelog.test.ts` bytes 6067–7577 (end exclusive),
lines 206–245, comprise the complete date-resolution block. Six tests in
`xtask/src/changelog/website.rs` preserve its original inputs and eight assertions:

- `source_dates_prefer_annotated_tag_and_fall_back_to_commit`
- `source_dates_preserve_recorded_date`
- `source_dates_fill_missing_from_lookup`
- `source_dates_legacy_heading_precedes_lookup`
- `source_dates_unresolved_version_remains_absent`
- `source_dates_recorded_keys_are_newest_first`

The original shared SAMPLE is retained as Markdown. Lookup callbacks preserve
the original constant, conditional and absent-return behavior; the source
NO_LOOKUP helper becomes a closure returning None. Complete ordered keys are
checked, not merely membership.

Pinned main passed six tests and eight assertions under Bun 1.3.14. Stable's
group passed five tests and five assertions: it does not contain the annotated
tag helper test. The native `changelog::website::tests::source_dates_` filter
passed six tests. Only this complete 1,510-byte test block is mapped; the runtime
generator and remaining tests are still incomplete.
