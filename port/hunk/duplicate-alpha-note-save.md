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

## Save-result compatibility

The composer now returns `Option<ReviewComment>`: a successful save returns the
persisted note, and a repeated save with no draft returns `None`. The fixed-clock
test checks both returned IDs, exact equality of the first result with stored
state, and the absent duplicate result. UI callers explicitly discard the result.
Edit saves resolve the existing target ID rather than the internal draft ID.

Both source pins passed the duplicate-save and user-note lifecycle cases together
(two tests, 28 assertions per pin). All 1,232 native TUI library tests passed in
8.67 seconds. The complete duplicate-save source-test interval is now mapped;
this does not map the remaining note lifecycle tests or the runtime hook.

Strict audit still fails on incomplete coverage: 1,257 baseline files, 1,433
interval records, 466 translated-test records, 277 unmapped records and 92 pending
upstream commits. Splitting this interval preserves one unmapped middle interval;
the unchanged unmapped-record count does not mean no source bytes were covered.
Formatting and diff checks passed. No full-port completion is claimed.
