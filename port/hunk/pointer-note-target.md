# Pointer note target preservation

The pinned hook case `moves the current line to the row a note is started on`
passed on both main 2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd in disposable Bun 1.3.14 checkouts
(one test, five assertions per pin).

The Rust test `source_controller::tests::pointer_note_start_moves_alpha_cursor_and_selection_to_note_line`
uses the same two-hunk alpha contents and targets new line 12 of hunk 1 through
the native note mouse action. It also checks unchanged line 9 with cursor painting
enabled and disabled. This is a controller-level pointer test: it supplies the
hit target directly and does not claim cell-level hit-testing coverage.

The additional line-9 case initially failed: when cursor painting was off, the
composer selected the hunk's default changed line 12, then replaced only the
draft's target with line 9. The selected cursor therefore disagreed with the draft.
The native composer now accepts an explicit target during initialization. Pointer
actions use that target for both draft creation and reveal; keyboard actions keep
their existing default-target resolution. The test checks draft, cursor, hunk,
side and line together.

This supplemental qualification does not map further source intervals or prove
the complete source note API, pointer geometry or terminal parity.

After the fix, all 1,191 TUI library tests passed (8.42 seconds). Formatting
and diff checks passed. No ledger disposition changed.
