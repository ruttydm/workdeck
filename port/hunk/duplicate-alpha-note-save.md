# Duplicate alpha note save

`source_controller::tests::duplicate_alpha_draft_save_persists_once_and_next_draft_has_unique_id`
uses the source alpha8 fixture, opens a native note draft, saves it twice without
an intervening frame, and verifies one persisted note with the intended body.
The next draft saves a second note with a distinct identifier and its own body.
The real composer consumes its draft synchronously on first save.

The source case `rapid duplicate saves persist exactly one user note with a unique id`
passed under disposable Bun 1.3.14 on main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (one test, fourteen assertions per pin).
The native focused test passed (0.75 seconds), plus formatting and diff checks.

This is not a complete source mapping: Hunk fixes Date.now and asserts
`user:<timestamp>-<sequence>` IDs and save return values. Native persisted notes
currently use `user-note-<sequence>` and save through a void composer method.
Identifier and callback-result compatibility remain explicit open requirements;
this test establishes only duplicate suppression, body preservation and uniqueness
within one live app. No runtime behavior or ledger disposition changed.
