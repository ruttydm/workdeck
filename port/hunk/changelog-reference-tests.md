# Changelog reference-definition test translation

The complete `link reference definitions` block in pinned main's
`scripts/generate-changelog.test.ts`, bytes 29637–31318 (end exclusive), lines
811–867, is translated in `xtask/src/changelog/website.rs`.

- `references_stay_out_of_last_entry` preserves the source fixture and compares
  the entire entry array, excluding legacy compare-link metadata.
- `references_stay_out_of_highlights` checks the exact retained Highlights text.
- `references_remain_inside_fenced_content` retains the source assertion that a
  reference definition inside a Markdown fence remains example content.

Both pinned source groups passed all three tests and three assertions under Bun
1.3.14. `cargo test -p xtask changelog::website::tests::references_` passed all
three native translations. Inputs, assertions and the source rationale are
preserved. The shared source fixture is inlined into its sole consuming test.

Only this complete 1,681-byte block is mapped. Surrounding test-file intervals
and the incomplete runtime generator remain unmapped. Splitting the old interval
adds one unmapped record while reducing remaining unmapped bytes.
