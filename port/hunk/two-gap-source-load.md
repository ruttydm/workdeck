# One source load, two expanded gaps

Baseline `src/ui/hooks/useTerminalReview.test.tsx` bytes 38,639–40,137,
lines 1179–1213, are translated by
`source_controller::tests::latest_gap_receives_cursor_when_one_source_load_reveals_two_gaps`.

The native fixture preserves all 50 before/after source lines, changes at lines
10 and 40, three context lines, TypeScript language, runtime ID `alpha`, and
no annotations. Embedded metadata is separate from the explicitly installed
unversioned loader. Channel synchronization replaces the source deferred promise
and React flush: the test observes the new-side invocation and loading status
before supplying the source text, then drains the real UI-thread completion.

Both gap slots must remain expanded. The final cursor must address the same
file, new side, and second hunk, at the start of its expanded leading gap.
This is the native source-line equivalent of the source assertion
`expandedGapKey === "before:1"`; the native cursor model has no string gap-key
field. The call channel must contain no second invocation.

The original named test passed on both pinned main and stable sources under
Bun 1.3.14 in disposable oracle trees: one test, twelve assertions, zero failures
per pin. Main total elapsed time was 481 ms; stable was 342 ms. These are test
durations, not benchmark measurements. The translated native test passed before
mapping. Surrounding tests, helper definitions, and the complete runtime hook
remain unmapped; this does not establish all lifecycle or terminal-frame parity.

Final scoped validation: all 1,181 TUI library tests passed in 11.26 seconds,
with zero failures, ignored tests, or filters. Formatting and diff checks pass.
Strict audit retains 1,257 baseline files and reports 1,401 records, 440
translated-test records, 271 unmapped intervals, and 11 cached upstream commits.
It exits with failure because the port is incomplete. The unmapped count rises
by one because the new mapped test separates two unfinished intervals.
