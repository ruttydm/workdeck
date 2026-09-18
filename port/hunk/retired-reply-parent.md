# Retired reply parent

The native composer previously allowed a reply save after its parent had been
removed. It now checks that the parent remains present and non-orphaned before
assigning a saved ID. Failure retains the draft ID/body and displays the pinned
core intent error, `Review note <id> is no longer available as a reply parent.`
Other storage errors also restore the composer rather than silently losing it.

`source_controller::tests::reply_save_rejects_removed_parent_and_retains_draft`
creates a note, opens a reply, removes the parent through the session interface,
and attempts to save. It verifies no new note or state revision, retained draft
identity/body and exact error text. This is supplemental runtime evidence; no
source interval is newly mapped. Cross-file parent validation and complete
semantic failure parity are not established by this regression.

The first implementation passed all 1,239 TUI library tests (9.62 seconds).
The subsequent preflight refinement additionally preserves the draft identifier
and uses the exact source error instead of a generic missing-comment error.
The refined focused regression passed in 0.80 seconds; formatting and diff
checks passed. The full suite was not rerun after that refinement.

## Cross-file reply guard

`reply_save_rejects_parent_from_a_different_file` constructs alpha and beta,
opens a reply to alpha's note, and deliberately retargets the draft to beta to
exercise save-boundary validation. The save now rejects that mismatch with
`Review note <id> belongs to a different file.` before assigning a saved ID.
The regression verifies preserved draft identity, unchanged parent and unchanged
state revision. Its source basis is pinned main `core/review/intents.ts`
lines 583–588. This does not claim a normal UI can construct that invalid draft;
it verifies the guard at the authoritative save boundary.
All 1,240 TUI library tests passed in 9.13 seconds, including both parent guards.
Formatting and diff checks passed. Ledger dispositions remain unchanged.
