# Changelog parser robustness test translation

The complete `parser robustness` block from pinned main's
`scripts/generate-changelog.test.ts`, bytes 21338–23670 (end exclusive), lines
584–657, is translated in `xtask/src/changelog/website.rs`.

The five native tests preserve all original inputs and nine assertions:

- `robustness_fenced_heading_preserves_following_section`: one release, both
  section titles, and the following section's pull-request identifier.
- `robustness_fence_closes_only_matching_delimiter`: one retained entry.
- `robustness_rejects_partial_version_heading`: malformed heading rejected and
  complete version accepted.
- `robustness_accepts_prerelease_heading`: exact prerelease version retained.
- `robustness_preserves_nested_sublist`: nested newline retained and flattened
  prose absent.

Both pinned source groups passed five tests and nine assertions under Bun 1.3.14.
`cargo test -p xtask changelog::website::tests::robustness_` passed all five native
tests. No original assertion was weakened or replaced by an invented fixture.
The source comment's rationale is retained alongside the native regressions.

Only this complete block is mapped. Surrounding test code and the unfinished
runtime generator remain unmapped. Splitting the interval increases the unmapped
record count by one while reducing unmapped bytes by 2,332.
