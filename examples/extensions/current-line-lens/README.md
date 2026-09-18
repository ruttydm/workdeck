# Current-line lens parity fixture

This native JSON-RPC extension translates Hunk's MIT-licensed
`test/pty/fixtures/current-line-lens/index.tsx` from baseline `2c00f435`.
It registers a fixed three-row bottom pane with the current-line capability: a clipped rule,
the old-side row, and the new-side row. Host-owned rendering supplies syntax and cursor context;
the pane is unavailable when the host has no split-view current line.

Build with `cargo build -p workdeck-examples --bin workdeck-example-current-line-lens-extension`.
Place that executable in this manifest's `bin/` directory when manually loading the fixture.
It is a test/example executable, not an additional shipped Workdeck command.

Run `cargo test -p workdeck-examples --test current_line_lens` for compiled extension, Unicode,
layout, availability, mouse-jitter, and copy-drag coverage. Frozen source-run metadata lives in
`port/hunk/oracles/pty-cursor-line.json`.
