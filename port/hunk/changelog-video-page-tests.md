# Original video embedding tests at page scope

Pinned main `scripts/generate-changelog.test.ts` bytes 25174–26370 (end exclusive),
lines 697–728, are now translated at complete-page composition scope.

The tests `source_video_page_contains_source_and_schema`,
`source_video_page_escapes_script_closing_title`, and
`source_video_page_escapes_attribute_breakout_url` retain the original SAMPLE,
date map, video overlays and all seven assertions. The helper invokes the same
native page composer used by the pages CLI, not the video fragment renderer.
All three native page tests pass. The earlier dual-pin source runs passed three
tests and seven assertions on each pin under Bun 1.3.14.

This closes the fragment-scope limitation previously recorded in the parser
progress document for this block only. Zola frontmatter, Workdeck naming and
Cargo-based installer content do not change the seven video assertions. Other
page tests, social cards and full-page differential evidence remain incomplete.
Only the complete 1,196-byte video test block is newly mapped; the runtime
generator remains unmapped.
