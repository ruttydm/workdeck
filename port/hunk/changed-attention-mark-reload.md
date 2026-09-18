# Changed attention-mark reload

Pinned hook test bytes 79221–80317 (exclusive end), lines 2356–2387, are translated
to `source_controller::tests::changed_alpha_reload_retires_attention_marks`.
The native test uses the source's twelve-line alpha fixture, adds a new-line-8
attention mark over [0, 6), confirms its presence, then reloads the source's
replacement changing alpha8 from 800 to 900. The mark map must become empty.
Both documents retain the source's embedded metadata and absence of a reader.

The source case passed under disposable Bun 1.3.14 on main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (one test, six assertions per pin).
The native changed-content and unchanged-content reload tests both passed.
Only this complete test body is mapped; source helpers and runtime implementation
remain separately accounted for.

All 1,202 TUI library tests passed (8.55 seconds); formatting and diff checks
passed. Strict audit remains incomplete: 1,257 files, 1,415 records, 276 unmapped
intervals and 11 cached pending upstream commits (exit 1).
