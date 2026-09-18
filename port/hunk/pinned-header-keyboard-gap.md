# Pinned-header keyboard investigation: non-asserting source wait

At parent commit `98d9a3d7`, a new native regression uses the pinned AppHost
collapsed-top fixture: 400 numbered lines, line 366 changed, original
`src/ui/components/panes/DiffPane.tsx` and `other.ts` paths, split 220×10 buffer.

`cargo test -p workdeck-tui --lib arrow_step_and_reverse_restore_collapsed_gap_beneath_pinned_header`
initially failed with default `CursorLineMode::Row`. The preceding cursor-off matrix case
passes Down and Up checks. In the failing row-cursor case, Down leaves `scroll=0`
and `current_line_row=3`; line 366 remains below the viewport. Follow-up source
instrumentation disproved the suspected product gap: pinned Hunk exhibits the
same row-mode state. The source requires line 366 visible only in cursor-off mode;
its row-mode test checks the collapsed gap/header count restored by Up, without
asserting the intermediate line visibility. The native test now preserves this
distinction, and additionally checks the observed row-mode cursor line and scroll.
The two source intervals are now individually mapped after corrected native
validation and fresh unmodified captures; see
[four-case capture](oracles/app-host-arrow-scroll.json). The containing test file
remains incomplete.

The corrected native test passed: 1 passed, 0 failed, 0 ignored, 1,109 filtered
out, 1.00 seconds. Workspace formatting, diff whitespace checks and TUI
all-target Clippy with warnings denied passed (Clippy 21.01 seconds).

Both unchanged pinned source tests passed using disposable Bun 1.3.14, null global
and system Git configuration, and a temporary Git cwd:

| Pin | First Down test | Down/Up test | Result |
| --- | --- | --- | --- |
| `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` | 329.71 ms | 558.60 ms | 2 pass, 74 filtered, 0 fail, 7 assertions |
| `4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` | 295.06 ms | 547.35 ms | 2 pass, 72 filtered, 0 fail, 7 assertions |

The source cases are bytes 62,070–63,288 (lines 2071–2111) and
63,288–64,958 (lines 2112–2165) of `src/ui/AppHost.interactions.test.tsx`.
Their interval SHA-256 values are respectively
`bc1a7efa471b149242d1da8e5e938314f3d1a8b7ecd7de4df6a36098b7ce1f03` and
`14dcc2aaf187e7addce1ee74a4cdb80cf9f72638b5b149b9f45e4d64138491a3`.

## Diagnostic observation

Temporary logging in the disposable main oracle showed the active cursor on
line 363 before Down and line 364 before Up. Reveal bounds were respectively
`top=3,height=1` and `top=2,height=1`, with viewport height 5, scrollTop 0 and
computed revealScrollTop 0 for both. A separate frame capture showed line 366
absent after Down. The source test still passed because `waitForFrame` (lines
455–474) returns the last frame after eight attempts, even if its predicate never
matches. The row-mode test does not assert that predicate afterwards.

These were supplemental diagnostic runs, not unmodified oracle captures. Logging
was removed with patches and all three files verified against pinned Git blobs:
App.tsx `9c5cdd1d1b469a4e99ce6ee2795e04bf7899371a`, DiffPane.tsx
`fe8b6f44c12c3eb2b8758421782e89bb18760fdc`, and the interaction test
`e7c707496160915b1caae48b1d1d5d214c34e948`. No runtime fix is justified by this
test. In particular, do not turn an unasserted polling predicate into a claimed
source guarantee.

## Traced paths

Source `App.tsx::stepDiffLine` scrolls when there is no active cursor, otherwise
calls `review.moveLineCursor`. The hook also reconciles initial cursors, so the
absence of a source cursor must not be assumed from the null initializer alone.
`DiffPane.tsx` applies `computeLineRevealScrollTop` to measured cursor bounds.

Native `step_diff_line` chooses the cursor route when cursor mode is enabled.
Its initial cursor and reveal outcome match the source diagnostic in this case.
The existing wheel regression alone was insufficient evidence; the separate
keyboard regression now records the actual source behavior.
