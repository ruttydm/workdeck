# Generated JSON test translation

Six source tests from pinned main `scripts/generate-changelog.test.ts` are
translated to the `source_formatter_*` tests in
`xtask/src/changelog/website/json.rs`.

The mapped byte intervals are `[35278,37118)` (lines 974–1012) and
`[37264,37405)` (lines 1017–1020), totaling 1,981 bytes. The 146-byte undefined
member test between them remains unmapped. The preceding social-card tests also
remain unmapped.

The translations preserve short inline arrays, expansion above 100 columns,
key-prefix width, the final-key comma exception, expanded object arrays and
compact empty containers. Original inputs and expected strings are retained.
Both per-line loops use UTF-16 counts, matching JavaScript string length. All
41 assertions belonging to these six tests remain executable in Rust.

The complete source group passed seven tests / 42 assertions in the disposable
pinned Bun runtime. The six translated Rust tests pass; the seventh source test
is explicitly excluded because Rust JSON has no undefined member value. The
formatter's separate 16 frozen comparisons remain supplemental evidence.

Source copyright is Modem Labs Inc., MIT; attribution is retained in the native
formatter and `THIRD_PARTY_NOTICES`. No runtime generator interval is mapped.

Strict audit reports 1,257 files, 1,458 intervals, 282 unmapped intervals and 92
queued upstream commits, and remains failing. The unmapped interval count rises
because the undefined test is isolated between completed intervals; unmapped
bytes decrease by 1,981. Strict xtask Clippy, formatting and whitespace checks pass.
