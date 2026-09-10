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

The initial supplemental test did not cover Hunk's fixed-Date.now identifiers
or save return values, so it did not receive a ledger mapping.

## Save-time identifier compatibility

New native notes now receive `user:<timestamp>-<sequence>` identifiers at save
time. A separate saved-note sequence prevents draft opening and editing from
consuming saved identifiers. The production clock supplies Unix milliseconds;
the same save implementation accepts an explicit timestamp for deterministic
tests. Existing persisted identifiers are never rewritten, and collisions with
already stored notes advance the sequence before insertion.

The duplicate-save test now asserts exactly `user:1700000000000-1` and
`user:1700000000000-2` with a frozen timestamp. The persisted-note collision
test uses that same timestamp across separate app instances and verifies that
the original note remains unchanged while the second receives suffix `-2`.
Extension save events are projected after assigning the persisted identifier.

Verification: the focused duplicate-save test passed, then all 1,232 TUI library
tests passed (8.62 seconds), including the fixed-clock persisted-ID collision
case. `cargo fmt --all -- --check` and `git diff --check` passed.

Save return-value compatibility is still open: the native composer method is
void. This source case remains unmapped; exact identifiers do not establish the
remaining callback-result assertions or complete lifecycle parity.
