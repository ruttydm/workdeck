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
