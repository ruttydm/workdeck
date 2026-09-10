# Session clear and human notes

`source_controller::tests::session_clear_alpha_human_notes_requires_explicit_opt_in`
recreates the two-hunk alpha fixture and the source session-clear sequence:
add an agent comment and save a human draft, remove the agent comment by ID,
add another agent comment and clear with default options, then add an agent
comment and clear with explicit `include_user: true`.

The test checks removal identity/source, all source removal-count assertions,
empty live-comment summaries, preservation of the complete saved human note
under default clearing, and empty comments/review summaries after inclusive
clearing. Existing runtime behavior passes; no production code was changed.

The pinned source test `session clear can include human user notes` passed on
main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`, one test and 25 assertions each,
using disposable Bun 1.3.14. The source test files were hash-checked against
the pinned Git blobs (`ed9cf33eed4e4a77a2ba4abf5d9f14b12df00635` and
`23517487f726d6b12cd8822cda7c7b57eb27fd16`, respectively).

The native focused test passed (0.76 seconds), as did all 1,211 TUI library
tests (8.72 seconds), formatting and diff checks. This evidence covers the
controller mutation sequence, not terminal
frames, session transport, or the complete note subsystem. Ledger dispositions
remain unchanged pending interval-level mapping review.
