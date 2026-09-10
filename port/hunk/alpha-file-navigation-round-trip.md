# Alpha file navigation round trip

`source_controller::tests::alpha_file_navigation_round_trip_clamps_header_alignment`
uses the source two-hunk alpha fixture followed by beta and gamma at 80 by four.
Starting at alpha hunk 1, it moves forward twice, attempts to move beyond gamma,
returns to alpha, and attempts to move before it. Each successful move selects
hunk 0 and aligns the file body top subject to the measured maximum scroll;
clamped moves preserve scroll and state revision.

Both pinned source versions passed `moves through visible files with clamped
file-header alignment` under disposable Bun 1.3.14 (one test, 28 assertions per
pin). The focused native test passed in 0.85 seconds; formatting and diff checks
passed.

This is supplemental, not a complete source-test mapping. Source assertions
also require exact `selectedFileTopAlignRequestId` values. Native state-revision
assertions are not substitutes for those reveal counters, and the source interval
remains unmapped. No runtime behavior changed.

## Reveal-intent integration

The TUI now applies the core `ReviewRevealIntent` reducer at the scroll-request
boundary. Rendering selects the requested anchor from changed file/hunk tokens
and consumes the intent's note-reveal flag. These are production reveal requests,
not aliases for the selection revision. The round-trip test now checks exact
file tokens 1, 2, 2, 3, 4, 4 alongside paths, hunk selection and scroll geometry.
The prior counter gap above is closed and this complete source case is mapped.

Both pinned versions passed the round-trip and counted cases together (two tests,
34 assertions per pin). All 1,250 TUI library tests passed in 9.13 seconds after
the reducer integration. Other reveal entrypoints and complete source hook parity
remain separate obligations; the runtime hook is not mapped by these tests.
The final explicit initial-hunk assertion passed in a focused rerun (0.74 seconds).
Strict audit still fails: 1,257 files, 1,440 intervals, 472 translated-test records,
278 unmapped records and 92 pending upstream commits. The unmapped count rises
because the two newly covered tests split their surrounding uncovered interval.
