# Index page test translation

The complete `describe("index page")` block in pinned main
`scripts/generate-changelog.test.ts`, bytes `[13066,14502)` / lines 371–405,
is implemented by `index_preserves_source_sample_content_in_zola_page` in
`xtask/src/changelog/website/index.rs`.

All five source tests' nine assertions are preserved in the combined Rust test:

| Source behavior | Native assertion |
| --- | --- |
| Only the newest series is Latest | Exactly one `Latest ·` occurrence |
| Counts releases and changes | Exact latest-series meta line |
| No factual fallback paragraph | Absence of the fallback for 0.15 |
| Intro has only off-page destinations | RSS present, CHANGELOG present, newest-first prose absent |
| Every series linked | Exact heading links for 0.19, 0.18 and 0.15 |

The fixture retains the original SAMPLE Markdown and dates. Branding and origins
are migrated to Workdeck. The intro delimiter changes from YAML `---` to Zola
TOML `+++`; this adapts the frontmatter boundary, not the asserted visible body.
An additional native assertion checks the explicit Zola URL path. The production
parser, grouping and renderer execute in this test; it is not fixture-only proof.

The source group passes five tests / nine assertions under the pinned disposable
Bun runtime. The Rust test passes all corresponding assertions plus the native
path check. Source attribution is MIT, copyright Modem Labs Inc., retained in the
implementation and `THIRD_PARTY_NOTICES`.

This maps exactly 1,436 source bytes. The preceding series-page test block and
the runtime generator remain incomplete; the shared SAMPLE declaration remains
in its separately tracked unmapped preamble.

Strict audit after mapping still fails with 1,257 baseline files, 1,455 intervals,
281 unmapped intervals and 92 queued upstream commits. The unchanged unmapped
interval count reflects shortening the existing interval, not unchanged byte
coverage: precisely 1,436 fewer source bytes remain unmapped.
