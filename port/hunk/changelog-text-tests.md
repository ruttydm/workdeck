# Original changelog text-helper test translation

Pinned main `scripts/generate-changelog.test.ts` bytes 7577–8520 (end exclusive),
lines 246–272, contain the complete text-helper block. Its five native translations
in `xtask/src/changelog/website.rs` are:

- `source_text_reduces_markdown_to_plain_text`
- `source_text_truncates_on_word_boundary`
- `source_text_leaves_short_text_alone`
- `source_text_quotes_frontmatter_like_source_formatter`
- `source_text_formats_iso_date`

All original inputs and seven assertions are retained, including literal Hunk
wording as test data and the exact single/double-quote escaping expectations.
Both pins passed five tests and seven assertions under Bun 1.3.14 with the
`^text helpers ` filter. The native `changelog::website::tests::source_text_`
filter passed five tests. The formatter-stability rationale is retained in the
native quoting test's comment.

Only this complete 943-byte test block is mapped. It does not establish complete
runtime-generator, metadata CLI, page-generation or website parity.
