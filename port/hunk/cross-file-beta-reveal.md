# Cross-file beta reveal qualification

`source_controller::tests::cross_file_beta_line_reveal_resolves_before_another_frame`
uses the source's one-line alpha change and thirty-line, three-hunk beta fixture,
including runtime IDs, paths, context and metadata-only snapshots without readers.
Starting on alpha, the real extension reveal handler resolves beta new line 30
immediately. Assertions cover file index 1, hunk 2, side/line, synchronized
selection, visible viewport placement and absence of an error status.

The source case `reveals one line of another file and asks for the reveal placement`
passed under disposable Bun 1.3.14 on main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (one test, fourteen assertions per pin).
The native regression passed without requiring an intervening painted frame.

This is supplemental qualification only. The source return value, exact reveal
counter/placement request and terminal-cell geometry still need their own
evidence. No source ledger mapping or production change is claimed.

All 1,206 TUI library tests passed (8.43 seconds), plus formatting and diff checks.
