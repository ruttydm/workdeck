# Alpha hunk reload clamp

Baseline hook test bytes `[10122, 11068)`, lines 313–339, are covered by
`source_controller::tests::reload_recovers_alpha_cursor_when_selected_hunk_is_retired`.
The fixture has twelve TypeScript lines and two changes (lines 1 and 12),
then reloads with only the line-1 change. The strengthened test checks the
selected file's two/one hunk counts and the stored selection's transition
from hunk 1 to hunk 0, in addition to the existing cursor recovery assertions.

The new stored-selection assertion initially failed: native selection was None
even though the synthesized current cursor pointed to hunk 0. The TUI reload
boundary now clamps a previously selected hunk to the selected file's remaining
hunks when core reconciliation cannot retain it. It does not synthesize hunk
selection for an explicit file-only selection or a file without hunks. Core
Workdeck reload semantics are unchanged.

The source case passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14:
one test and ten assertions per pin. Native owned state replaces React
flush/destroy scaffolding; this source case has no frame assertions.
After the fix all 1,222 TUI library tests passed (9.08 seconds), with formatting
and diff checks passing. The runtime hook remains unmapped.

Strict audit validated ledger structure and evidence before rejecting incomplete
coverage: 1,257 files, 1,427 records, 461 translated-test records, 276 unmapped
records and 92 pending upstream commits. Mapping 946 interior bytes splits one
remaining interval into two; the interval count is not a completion percentage.
