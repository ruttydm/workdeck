# Hunk semantic-port ledger

Ledger mutations hold a nonblocking OS lock for the entire read/modify/write transaction and
replace the ledger from a unique, flushed temporary file. A competing writer fails with a retry
message instead of sharing a temporary file or losing another mapping. The ignored
`ledger.jsonl.lock` sidecar remains on disk deliberately; the OS releases its lock when the owner
exits. Read-only status/audit commands do not acquire or create this sidecar.

Workdeck ports the pinned Hunk source tree without merging Hunk's unrelated history into the
Workdeck mainline. The source anchors are:

- `hunk-port/main-2c00f435` (`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`)
- `hunk-port/stable-v0.20.1` (`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`)

The `hunk-upstream` remote fetches branches and tags into namespaced refs. The tracked ledger is
generated from Git blobs, not from a vendored TypeScript source tree:

```console
cargo xtask port inventory
cargo xtask port status
cargo xtask port audit --allow-incomplete
cargo xtask port map --path PATH --disposition rust-reimplementation \
  --destination crates/... --evidence crates/...
```

`audit` without `--allow-incomplete` is the release gate. Every baseline byte must be covered by
exactly one ledger interval. A mapped interval must name both repository destinations and test or
verification evidence. Valid dispositions are Rust reimplementation, translated test, migrated
content, retained asset, Rust-generated replacement, and retained license.

Port commits name their records with `Hunk-Port:` trailers. Stable and catch-up commits additionally
carry `Hunk-Upstream:` trailers. An unmapped record is visible work, never an implicit waiver.

The five commits unique to Hunk `v0.20.1` are tracked separately in
`port/hunk/stable-fixes.jsonl`; the four functional regressions have Rust implementations and
named tests. They do not falsely mark the larger baseline blobs containing those files as ported.

## Terminal lifecycle verification

`oracles/pty-lifecycle.json` records the five passing baseline oracle cases and the test file's
absence from the stable pin. Native CLI subprocess tests now cover private PTY closure, macOS
controlling-terminal revocation, and the three shutdown signals with both terminal streams and
broker-enabled pipes. A dropped
terminal returns success after session retirement, while non-terminal-I/O errors remain failures.
The test harness owns and reaps its children and closes inherited PTY handles explicitly.

Redirected review output uses the upstream 80-by-24 fallback. On Unix, a private raw PTY forwards
redirected input to Crossterm's existing parser without creating a controlling terminal or process
group. Its stoppable worker and saved stdin descriptor are owned by the interactive terminal guard.
Mouse capture follows the original terminal interactivity, not this internal adapter.

The five source cases have named native tests in the oracle record. Pipe signal cases retain the
source's two-second review exit deadline; the separately owned daemon has a six-second cleanup
deadline. Additional tests cover terminal signals and redirected arrow/quit input. These lifecycle
assertions do not establish broader cell-buffer visual parity or complete the surrounding UI files.

Run the current native coverage with `cargo test -p workdeck-cli --test terminal_lifecycle`.

`oracles/pty-key-routing.json` maps all seven key-ownership cases from both pins to Ratatui
frame/state tests (`cargo test -p workdeck-tui pty_key_routing`). They preserve row-20 review anchors
under menus and theme navigation, filter text/focus under Escape and menu Enter, and note-editor
ownership of F10. The theme controller carries forward its last rendered window before keyboard or
hover preview transitions, avoiding unintended list recentering.

Cursor-line evidence is in `oracles/pty-cursor-line.json`: both upstream pins pass all
11 cases; seven have native frame/state translations and four use the compiled native lens fixture
and Ratatui mouse events (`cargo test -p workdeck-examples --test current_line_lens`). Paging updates the semantic selection,
not just its screen row. Drafts use the existing inline-note painter and insert real review rows
after their target, retaining downstream file/hunk/note geometry. Expanded source rows receive
cursor/note targets; selecting them validates the retained source snapshot through an explicit
review API without relaxing ordinary changed-hunk `reveal_line` validation. Gap expansion remembers
and restores the previous cursor target. The lens retains old-above-new rendering, fixed bottom-pane
geometry, Unicode text, and split-only availability. Mouse tests cover one-cell click jitter,
post-paging line selection, and multi-row copy drags across highlighted repaints.
