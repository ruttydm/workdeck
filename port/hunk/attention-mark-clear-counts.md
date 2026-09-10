# Attention-mark clear counts

Pinned hook bytes 75739–77509 (exclusive end), lines 2253–2310, are translated
to `source_controller::tests::attention_mark_clear_counts_preserve_other_files`.
The test uses the source's two-hunk alpha and three-hunk beta TypeScript fixtures,
with metadata-only snapshots and no readers. It adds marks on alpha lines 1/12
and beta line 1, each over [0, 4). Clearing alpha must remove two and leave one,
report alpha.ts and retire alpha's key without changing beta's mark. Global
clearing must remove one and leave zero, omit the file path and empty the map.

The source case passed under disposable Bun 1.3.14 on main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (one test, nine assertions per pin).
The native regression passed. This maps only the complete clearing test body,
not fixture helpers or surrounding runtime code.

All 1,204 TUI library tests passed (9.32 seconds); formatting and diff checks
passed. Strict audit still exits 1 with 275 unmapped intervals across 1,257
files and 11 cached pending upstream commits. This closes one previously
unmapped interval without changing the baseline file count.
