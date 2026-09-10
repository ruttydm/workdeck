# Review interaction helper translation

Source: Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`,
`src/ui/AppHost.interactions.test.tsx`, MIT, copyright Modem Labs Inc.
See `THIRD_PARTY_NOTICES`.

The native `open_themes_modal_from_view_menu` test helper in
`crates/workdeck-tui/src/lib.rs` translates the source helper's F10,
rendered File-menu assertion, Right, render, `t`, and rendered Theme-selector
assertion. Native key handling and Ratatui TestBackend rendering are synchronous;
this helper does not emulate OpenTUI scheduler sleeps or prove asynchronous
terminal/session behavior.

Executable evidence:

- `cargo test -p workdeck-tui --lib custom_theme_stays_active_when_opened_through_view_menu`
  verifies the custom theme stays selected and retains its accent.
- `cargo test -p workdeck-tui --lib view_menu_theme_selector_opens_at_representative_terminal_sizes`
  exercises the same menu path at 40x12, 80x24, 140x32, and 220x20, then
  verifies Escape dismisses the dialog without changing theme identity or accent.

Both tests passed locally on macOS arm64. These assertions inspect native
cell-buffer text and application state, not frozen upstream terminal frames.
They do not establish geometry parity or cross-platform success.

The adjacent native `press_hunk_navigation_key` helper dispatches each `[` or
`]` separately and renders after each key, as the pinned helper does.
`first_cross_file_hunk_navigation_header` preserves first-match ordering,
whitespace trimming, and the source's empty-string fallback (the previous
inline native closure panicked when no header was present).

`cross_file_hunk_sequence_preserves_destination_header_and_backward_target`
uses both helpers against the native review stream: 18 forward steps enter
the short file, one more reaches its middle hunk, and two backward steps
return to the last long-file hunk rather than its first hunk.
`cross_file_header_helper_preserves_first_match_and_empty_fallback` directly
tests ordering, trimming, and missing-header behavior.

After these translations, `cargo test -p workdeck-tui --lib` passed all
1,261 tests with zero failures and zero ignored tests on macOS arm64.

The source interval at bytes `[15678,17425)` also owns the
snapshot-waiting helper. It remains unmapped; this partial translation must
not be used to claim completion of that interval or the whole source file.
