# Alpha selection document identity

`source_controller::tests::selection_only_alpha_navigation_retains_document_allocation`
uses the source two-hunk alpha fixture, retains the immutable changeset Arc,
selects hunk 1 through the live controller, and checks the stored selection,
pointer equality of the changeset, unchanged source identity and generation.
The focused test passed (0.75 seconds), with formatting passing.

The source case `keeps review stream identities stable across selection-only navigation`
passed on main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14,
one test and five assertions per pin.

This is deliberately not a full source-test mapping. Hunk asserts reference
identity of the filtered visible-files array, whereas this native test proves
identity of the backing immutable document. Visible projection allocation and
identity behavior require separate evidence. No ledger disposition or runtime
implementation changed; passing this test does not establish rendering or
performance parity.

All 1,225 TUI library tests passed (9.26 seconds). Diff checks passed.
