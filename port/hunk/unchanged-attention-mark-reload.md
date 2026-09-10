# Unchanged attention-mark reload

Pinned hook test bytes 77509–79221 (exclusive end), lines 2311–2355, are translated
to `source_controller::tests::unchanged_alpha_reload_rekeys_attention_marks_and_clear_counts`.
The test preserves the source alpha8 fixture and no-reader metadata, adds the
new-line-8 range [0, 6) with default match tone, reloads unchanged contents under
`alpha-reloaded`, verifies exact mark preservation and retirement of the old key,
then checks global clearing returns one removal, zero remaining and no file path.

The source case `a reload keeps agent attention marks on files whose content is unchanged`
passed under disposable Bun 1.3.14 on main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (one test, nine assertions per pin).
Only this complete test body is mapped, not the shared fixture helpers or the
surrounding clear-count and changed-content cases.

All 1,201 TUI library tests passed (8.41 seconds); formatting and diff checks
passed. Strict audit still exits 1 with 276 unmapped intervals, 1,414 records,
1,257 files and 11 cached upstream commits. Extracting the covered body splits
one unmapped interval into two; no previously covered bytes were removed.
