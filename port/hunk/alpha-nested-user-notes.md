# Alpha nested user notes

`source_controller::tests::alpha_saved_notes_edit_in_place_and_form_nested_reply_chains`
translates the pinned main hook test with the alpha8 fixture. It creates a root,
edits it without changing the returned ID, replies to the root and then to its
child, and checks the complete ordered summary/body/parent projection and three
distinct IDs. Native active-note targeting uses the saved-note ID selection used
by the mouse route before invoking the real edit/reply composer actions.

The first native run exposed a behavioral gap: session removal deleted the root
despite surviving replies. Individual session removal now rejects notes with
children before mutation, using Hunk's error text. Mouse deletion uses the same
checked route and displays errors. Bulk clearing retains its separate semantics.
Additional assertions verify unchanged state revision on rejection and successful
leaf-first removal of the chain.

Pinned main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` passed this source
test under disposable Bun 1.3.14 (one test, ten assertions). The test is absent
from pinned stable `4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`; the attempted
stable filter matched zero tests, not a passing parity run.

Only main test bytes 28346–30319 (lines 891–946) are mapped. The following
mouse viewport-anchor test and the runtime hook remain unmapped.

After the guard and leaf-first cleanup assertions, all 1,233 native TUI library
tests passed in 8.78 seconds. Formatting and diff checks passed.
Strict audit reached the incomplete-coverage gate: 1,257 files, 1,434 interval
records, 467 translated-test records, 277 unmapped records and 92 pending upstream
commits. It failed as expected on unmapped records; this is not release parity.
