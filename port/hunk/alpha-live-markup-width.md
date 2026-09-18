# Published live markup width

`source_controller::tests::alpha_markup_validation_uses_published_layout_width`
translates the pinned hook test using the single-line alpha fixture, experimental
launch authority and published review width 120. The first stack-layout comment
reports markup width 112. Switching the same app to split layout before the next
comment reports a width below 70. Both use `<box border>ok</box>` and disable reveal.

Both pinned main and stable source cases passed under disposable Bun 1.3.14
(one test, four assertions per pin). The initial focused native run passed in
0.77 seconds; its second summary was then aligned with the source's `Docked note`.

Only source hook-test bytes 18025–19832, lines 539–599, are mapped. This checks
the published geometry consumer, as the source test does; it does not prove the
complete terminal resize or split-dock rendering matrix.
All 1,249 TUI library tests passed in 9.10 seconds with the final test input.
Formatting and diff checks passed.
Strict audit remains incomplete: 1,257 files, 1,437 interval records, 470
translated-test records, 277 unmapped records and 92 pending upstream commits.
