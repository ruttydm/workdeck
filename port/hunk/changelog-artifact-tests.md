# Artifact and pre-tag source tests

Pinned main `scripts/generate-changelog.test.ts` bytes `[17853,21338)`, lines
503–583, contain the complete artifacts and pre-tag-window groups. All seven
tests are translated in `xtask/src/changelog/website/artifacts.rs`:

- `source_artifacts_include_series_index_feed_and_data` checks the seven original
  destination suffixes against the native artifact map.
- `source_artifacts_record_newest_published_release` preserves the newest-version
  and minor-series checks.
- `source_artifacts_unpublished_changesets_have_no_latest` uses the original
  all-Changesets input, avoiding the legacy heading date that would imply release.
- `source_artifacts_are_identical_on_repeated_generation` compares complete maps.
- `source_pretag_index_marks_published_series_latest` checks all three original
  assertions on the first two series.
- `source_pretag_feed_excludes_unreleased_series` checks both feed membership rules.
- `source_pretag_landing_retains_published_release` checks the published version
  and minor series during preparation.

Source SAMPLE, dates and lookup-disabled semantics are unchanged. Branding and
native destination roots are migrated; the original final two path components
are retained. Source `toMatchObject` assertions become separate field assertions,
so all 16 source expectations are preserved as 18 Rust assertions. No generated
file is written: like the source function under test, the native generator returns
an artifact map.

The pinned source groups pass seven tests / 16 expectations. The native artifact
module passes nine tests, including these seven, the composition test and the
dual-pin exact JSON/RSS comparison. Attribution remains MIT Modem Labs Inc. in
the implementation and notices. This maps 3,485 test bytes, not the runtime
generator or remaining website/release gates.

Strict audit remains failing with 1,257 files, 1,458 intervals, 281 unmapped
intervals and 92 queued upstream commits. Strict xtask Clippy, formatting and
diff whitespace checks pass after the translation.
