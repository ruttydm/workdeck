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
