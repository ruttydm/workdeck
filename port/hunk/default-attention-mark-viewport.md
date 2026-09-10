# Default attention-mark viewport qualification

`source_controller::tests::default_alpha_attention_mark_does_not_move_viewport_or_selection`
uses the pinned two-hunk alpha fixture, moves the cursor away from the target,
then adds new-line-1 range [13, 18) without a tone or reveal option. It verifies
default match tone, hunk 0, one mark, absent reveal result, exact stored payload,
and unchanged cursor, document selection and scroll.

The related source test `defaults agent marks to the match tone without moving the viewport`
passed under disposable Bun 1.3.14 on main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (one test, six assertions per pin).
The native regression passed.

No new source mapping is claimed: the source asserts a stable reveal-request
counter, whereas this native test directly checks state and viewport stability.
Counter/lifecycle evidence remains an open parity requirement. Production runtime
code is unchanged by this qualification.

All 1,205 TUI library tests passed (8.83 seconds); formatting and diff checks
passed. Source ledger dispositions are unchanged.
