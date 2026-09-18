# Normal-session markup rejection

`source_controller::tests::alpha_normal_session_rejects_single_and_batch_markup_atomically`
translates the complete pinned hook test using alpha8. It submits a new-side
line-8 single comment and a hunk-0 batch comment with `<badge>hidden</badge>`
markup, verifies both errors require relaunching Workdeck with `--experimental`,
and checks that saved/live comments remain empty. It additionally verifies the
state revision is unchanged.

Both main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` passed the source case under
disposable Bun 1.3.14 (one test, six assertions per pin). The native focused
test passed in 0.78 seconds. Branding is changed from Hunk to Workdeck as required.

Only source-test bytes 21306–22449, lines 653–692, are mapped. The preceding
live-width and degraded-markup source tests remain unmapped.
Formatting and diff checks passed. Strict audit still fails on incomplete
coverage: 1,257 files, 1,435 intervals, 468 translated-test records, 277 unmapped
records and 92 pending upstream commits. This is not full-port completion.
