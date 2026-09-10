# Alpha user note lifecycle

`source_controller::tests::alpha_user_note_save_projection_and_removal`
uses the single-line alpha fixture at native dimensions 80 by four. It creates
and saves a draft, checks one stored note, then checks session-facing ID, user
source, file path, hunk 0, new range [1, 1], body, and editability. Removal by
ID must report success, user source and zero remaining comments; both stored
comments and review summaries must then be empty.

The source case `user note drafts can be saved, removed, and exposed as review notes`
passed under disposable Bun 1.3.14 on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`: one test and fourteen assertions
per pin. The native focused test passed (0.82 seconds), with formatting and diff
checks passing.

This is not a complete source-test mapping. Source IDs start with `user:` and
save returns the saved note; native TUI IDs currently start with `user-note-`
and `save_note_composer` returns no value. The native test retrieves its ID from
state, which does not prove callback-result compatibility. These remain explicit
port obligations. No runtime or ledger disposition changed.
