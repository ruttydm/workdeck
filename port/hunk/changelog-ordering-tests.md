# Changelog prerelease ordering and publication tests

Pinned main `scripts/generate-changelog.test.ts` bytes 23670–25174 (end exclusive),
lines 658–696, contain two complete describe blocks now translated in
`xtask/src/changelog/website.rs`:

- `prerelease_comparison_has_defined_order_for_source_nan_regressions`
- `prerelease_ordering_places_stable_above_unnumbered_channel`
- `prerelease_ordering_compares_channels_and_numbers`
- `published_policy_preserves_dated_prereleases_without_stable_promotion`

The first source test checks finite numeric results for three version pairs.
Rust returns `Ordering`, which cannot represent NaN; the translation additionally
checks the exact order for those same three pairs. Both sorting tests retain
their input and complete expected arrays. The publication test retains the two
release models, date maps and all five original boolean assertions.

Both pins passed three ordering tests and five source assertions. Pinned main
also passed its publication-policy test and five assertions. Stable's publication
policy intentionally differs and is not claimed to match main's five assertions.
The native ordering filter passed three tests and the publication-policy filter
passed one. Source tests ran under Bun 1.3.14; native tests used `cargo test -p
xtask changelog::website::tests::prerelease_` and the `published_policy_` filter.

Only these complete blocks are mapped. Remaining generator tests and runtime
implementation remain incomplete. This maps 1,504 bytes without changing the
number of unmapped intervals.
