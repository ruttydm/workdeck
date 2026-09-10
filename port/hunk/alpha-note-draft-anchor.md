# Alpha note draft anchor

`source_controller::tests::alpha_mouse_targeted_note_drafts_preserve_cursor_and_scroll`
uses the alpha8 source fixture, saves a root note, moves the cursor two lines,
and targets that note for editing and then replying, with a real Escape cancel
between the drafts. Both openings preserve the logical line target, selection
and scroll offset. The focused native test passed in 0.79 seconds.

The first test attempt compared native `ReviewLineCursor.row` too; reply insertion
changed that rendered row from 8 to 12 while retaining new-side line 7. Pinned
Hunk's `LineCursor` has a file ID, hunk index, stable key and line target, not an
absolute screen row. The corrected assertion compares the native logical target.
This is not a normalization of terminal parity fixtures: no terminal-frame
comparison or row-geometry equivalence is claimed by this supplemental test.

Pinned main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` passed the source
`mouse-targeted edit and reply drafts preserve the current viewport anchor`
case under disposable Bun 1.3.14 (one test, thirteen assertions). The named
test is absent from pinned stable's hook-test file.

The source also asserts preservation of the complete reveal request with
`scrollToNote: false`, and exact logical stable-key identity. This native test
does not yet cover those assertions or rendered mouse-hit routing. Bytes
30319–32187 remain unmapped. No runtime behavior changed for this test.

All 1,234 native TUI library tests subsequently passed in 9.71 seconds. Formatting,
diff checks and strict workspace/all-target Clippy passed; the latter required
fixing two unnecessary single-item-slice clones in an earlier batch-reveal test.

## Keyboard and pointer cursor distinction

Pinned `startUserNoteEdit` and `startUserNoteReply` explicitly apply the draft's
line cursor unless `preserveViewport` is set (hook lines 1341–1394). Native
keyboard/catalog edit and reply actions previously called the same preserving
open methods as mouse actions without restoring the note's line.

`alpha_keyboard_note_actions_restore_the_note_line_target` exposed that gap:
after moving two lines beyond a saved root, keyboard Edit left new-side line 7
selected rather than the root's line 5. Both keyboard actions now resolve the
draft's exact target in the measured row plan and apply that cursor. Pointer
actions retain their preserving route. The test covers Edit and Reply, with a
real Escape cancellation between them.

This fixes cursor targeting only. Complete default note reveal placement and
the source reveal-request/stable-key assertions remain open; no source interval
is newly mapped by this change.
All 1,236 TUI library tests passed in 8.77 seconds after the cursor fix, including
the separate pointer-preservation test. Formatting and diff checks passed.

## Keyboard composer visibility

The same keyboard regression now uses an 80-by-8 review viewport and an initial
scroll offset of zero. Its first run after adding geometry assertions failed:
Edit left scroll at zero where the draft bounds required offset three. The
keyboard route now uses `compute_line_reveal_scroll_top` on the draft's measured
note bounds, capped by the stream's maximum scroll, after restoring its cursor.
The source basis is `DiffPane.tsx` lines 2198–2216, which reveals default drafts
using `computeLineRevealScrollTop`, and `hunkScroll.ts` lines 76–107.

The test checks required scrolling for both Edit and Reply. Mouse-targeted draft
opening remains separate and preserves its offset. Complete source stable-key,
reveal-request and prior viewport-anchor reconciliation remain unproven; no
additional ledger interval is mapped.
All 1,236 TUI library tests passed in 8.70 seconds after this change; formatting
and diff checks passed.

## Viewport-preserving reveal intent

After reveal intents were integrated into scrolling, successful viewport-preserving
Edit and Reply openings now submit the core viewport-anchor request. It clears
`scroll_to_note` without incrementing file/hunk reveal tokens or moving scroll.
The pointer-preservation test seeds a prior note-reveal flag before each opening
and asserts the complete resulting intent, alongside unchanged cursor target,
selection and scroll. Rejected opens do not submit a reveal request.

Stable-key identity and the complete keyboard/default draft reveal protocol
remain separate obligations. This extension does not yet map the source interval.
All 1,250 TUI library tests passed in 9.23 seconds; formatting and diff checks
passed.

## Keyboard draft intent

Successful keyboard Edit/Reply now applies the core `REVIEW_DRAFT_START_REVEAL`
request before resolving the composer geometry. This increments the hunk reveal
token, preserves the file token, and sets the note-reveal flag, which the composer
visibility calculation consumes. The keyboard test checks all three fields for
each action as well as the restored line target and narrow-viewport scroll.
Pointer opening still uses the viewport-preserving request. This does not yet
cover every source stable-key or default new-draft reveal path.
All 1,250 TUI library tests passed in 9.22 seconds; formatting and diff checks
passed. No ledger disposition changed.
