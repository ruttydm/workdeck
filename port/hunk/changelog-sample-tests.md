# Shared changelog parsing and Highlights test translation

Pinned main `scripts/generate-changelog.test.ts` lines 132–205 contain the complete
`changelog parsing` and `highlights` blocks, translated into eleven native tests
prefixed `source_sample_` in `xtask/src/changelog/website.rs`.

`website-changelog-test-sample.md` retains the original SAMPLE Markdown, with only
the TypeScript template-literal backtick escapes decoded. Its Hunk wording and
URLs intentionally remain unchanged as test inputs. Original release ordering,
prerelease flags, legacy date, Highlights extraction, empty-section omission,
minor-series ordering, lead/body separation, editorial precedence and absent
summary assertions are all retained. Each original test has its own native test.

Both pinned source groups passed eleven tests and fourteen assertions under Bun
1.3.14 using the filter `^(changelog parsing|highlights) `. The native filter
`changelog::website::tests::source_sample_` passed eleven tests.

The shared source fixture is used by these tests, but its source declaration and
surrounding imports remain in the unmapped preamble interval. Only the two
complete test blocks are newly mapped. Other source tests and the runtime
generator remain incomplete.
