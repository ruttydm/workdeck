# Hunk semantic-port ledger

## Immutable native review documents

`ReviewState` owns its changeset through `Arc<Changeset>`. Its `changeset()` accessor remains
read-only, and `changeset_snapshot()` retains that exact immutable document. State clones share
the document; selection, layout and comments remain independently owned state. Reload installs
a new allocation. A fresh replacement state is distinguishable even if its generation and file
IDs equal an older state's values. A retained `Arc` prevents allocation-address reuse while a
consumer uses its identity. Caller copy-on-write edits cannot mutate the state-owned document.

The public `ReviewSnapshot` still contains an owned `Changeset`; its serialized schema is not
changed. This ownership boundary enables future geometry reuse but does not itself implement
a geometry cache or prove a performance gate. Such reuse must also account for layout, width,
notes, expanded gaps, filtering and extension geometry; generation alone is insufficient.

## Ledger transactions

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

Status/audit list the tracked post-baseline commit hashes in reverse topological order (parents
before children). Strict audit rejects missing upstream refs as unknown, as well as a non-empty
delta. These observations use the locally fetched `hunk-upstream/main`, not a live remote query;
the final fetch requirement remains mandatory. Currently this is the raw post-baseline range:
there is no verified catch-up disposition registry yet, and commit trailers alone do not remove
entries. Implementing and verifying that registry is outstanding; changing the baseline or
moving the upstream ref backward is not an acceptable way to clear this gate. Discovery requires
the tracked upstream tip to descend from the baseline and rejects unrelated or truncated history.
A temporary-repository test verifies missing/equal tips, two-parent ordering, and rejection of
rewound and unrelated tips without modifying the real upstream refs.

The five commits unique to Hunk `v0.20.1` are tracked separately in
`port/hunk/stable-fixes.jsonl`; the four functional regressions have Rust implementations and
named tests. They do not falsely mark the larger baseline blobs containing those files as ported.

## Migrated release-fragment history

All 77 pinned release-note fragments are migrated as historical documentation in
[`release-fragments.json`](release-fragments.json) and the generated
[upstream release-fragment history](../../docs/upstream-release-fragments.md). This includes 34
versioned notes and 43 maintenance-only fragments, including maintenance entries with prose.
Each path, blob identity, version-bump metadata, and body is retained. The Rust generator
reconstructs every source byte and compares it with the pinned Git blob before rendering or
checking the document. Negative tests reject changed text, changed bumps, altered blob IDs,
and omitted fragments. `cargo xtask verify` includes this check.

Run `cargo xtask changelog upstream-history` to emit the document to stdout, or add `--check`
to verify the committed document without writing. These historical quotations are not Workdeck
release claims or current runtime/install requirements. The separate Changesets README,
configuration, and prerelease state remain unmapped until the native release workflow is ported.
The documentation mappings do not complete any described runtime feature or release gate.

Native [release-channel and version policy](../../docs/native-release-policy.md) is available through
`cargo xtask release channel` and `cargo xtask release check-version`. The former preserves
latest/beta/backport/manual selection while returning native `channel` metadata instead of an
npm publication tag; the latter verifies the exact tag against the executable's Cargo version.
All seven source channel tests are translated, and `oracles/release-channel.json` records 18
matching differential cases from both pinned runtimes. No command publishes or mutates a release.
Generated prerelease state can be checked separately with `cargo xtask release validate-prerelease`.
All 15 source validation, path-classification, and PR-routing cases have native translations;
`oracles/pr-release-notes.json` records both complete upstream runs and their named mappings.
`cargo xtask release verify-pr-notes BASE_REVISION [HEAD_REVISION]` selects generated validation
only for metadata-only preparation with existing prerelease state, otherwise the ordinary gate.
The native ordinary-fragment gate is `cargo xtask release status --since=REVISION`. Real Git tests
cover merge-base selection, modified/deleted/untracked fragments, maintenance entries, hidden and
README filtering, unknown packages, and read-only behavior; YAML tests cover all 77 pinned inputs,
anchors, merges, and duplicate rejection. Stable promotion and ordinary changes exercise the
actual native status route; invalid generated state fails without bypassing validation.

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

Chrome evidence is in `oracles/pty-chrome.json`: both pins pass all nine cases and 43 upstream
assertions. `cargo test -p workdeck-tui pty_chrome` exercises routed mouse/keyboard menus, theme
selection and rapid previews, note visibility, isolated preference saving with delayed quit,
filter focus, controls help, and outer-cell gutter colors. Live rows now derive line-number widths
per file instead of reserving four digits; measurement uses the same default. Exact stacked gutter
text remains an assertion, not an allowed visual normalization. Broader UI source files and full
cross-renderer cell-buffer/performance parity remain incomplete.

Pager evidence is in `oracles/pty-pager.json`: both pins pass 11 tests and 61 upstream assertions.
`cargo test -p workdeck-cli --test terminal_pager` runs the compiled executable in owned Unix PTYs
with a Rust VT parser, real keyboard/mouse bytes, file-backed patch stdin and a separate real-pipe
input case. Pager defaults are applied at the shared live/frame constructor, including an initially
hidden sidebar despite `--sidebar`; users can reveal both sidebar and menu. Redirected stdin uses
the controlling terminal for raw input and mouse capture. Darwin automatic-theme probing uses
`select` for the `/dev/tty` alias. Harness cleanup closes the PTY master before reaping a killed
child and drains restoration output during normal quit. Windows native validation remains a
separate release gate; this translation does not claim complete terminal-cell differential parity.

`oracles/pty-session-attention.json` captures the single end-to-end attention test from both pins
(15 upstream assertions each). Its native test lives alongside the pager PTY harness and owns a
private loopback daemon. Session highlight/navigate now scroll to the selected line's rendered row,
not the owning hunk's start; the test checks line 111's near-top landing, real warning-mark cell
backgrounds, clear counts, and navigation back to line 25. Terminal output continues draining during
session CLI exchanges. Color tests explicitly enable truecolor without inheriting `NO_COLOR`.

Verification follow-up (2026-09-08): a workspace run reported an intermittent broker authentication
failure in this attention test (87/88 terminal tests passed). An isolated rerun and the next
workspace terminal run passed; the cause is not established and a retry is not a resolution.
Another freshly rebuilt isolated run hit the five-second private-daemon readiness deadline, then
passed on retry. Failure diagnostics now identify the exact session subcommand and include the
private daemon's startup output on readiness failure; neither deadline has been increased.
Separately, a new listener test reproduced empty connections retaining a worker until the
five-second HTTP-header deadline; EOF
now retires those connections immediately. That fix is not asserted to explain the authentication
failure. No parity or release gate is waived by these observations.

`oracles/pty-notes.json` records all 19 baseline and 18 stable oracle passes (the threaded-action
test is baseline-only), with nineteen passing native translations. Live pointer movement controls add-note affordances;
composer Save/Cancel hit areas own their mouse events. Agent annotations use the shared note painter
after their source anchor, including collapsed-gap ownership and experimental STML bodies.
Draft titles receive the owning file, LF/Ctrl+J inserts a newline, cursor-off
drafts reveal the default hunk target, and split composer insertion recognizes either side of a
paired row. Saved user cards share the note painter, expose hover-only action hit areas, and edit
in place with a cursor at the beginning. Replies use the shared visible-thread selector;
pending sibling drafts update branch guides without entering the persistent store. The native test
covers nested/sibling replies, keyboard edit/reply and leaf deletion. This completes the notes test
file, not the broader renderer implementation or full terminal-cell differential gate.

`oracles/pty-layout.json` records 23 baseline and 18 stable layout oracle passes and names every
native translation. Tests operate the compiled binary in private PTYs, resize both the actual
terminal and VT parser, and inspect real repository diffs. They cover viewport filling, gaps,
Unicode paths/cell widths, tab widths, wrap/context toggles, responsive layouts, sidebar policy
and drag projection changes, anchored navigation, and horizontal arrows/wheel input. Wrapped split
rows reserve the canonical three-cell add-note lane; wrap toggles reset horizontal offset and
compact sidebar group headers retain their padding. Continuation rows preserve both diff rails.
These source assertions do not replace the full terminal-cell oracle comparison release gate.

The shared native PTY harness is being translated separately and remains unmapped. Review frames
now carry synchronized-output boundaries, and snapshot predicates wait until an update is complete.
The shared drag helper emits the source's press, five interpolated motion steps, and release rather
than collapsing a drag into one motion. Its coordinate tests cover both directions, rounding,
and stationary input; existing sidebar drag tests exercise it against the executable. Polling is
capped by the remaining wait deadline so short motion intervals are not expanded to a fixed 50 ms.
Fixture factories and the rest of `test/pty/harness.ts` still require complete source accounting.
`oracles/pty-harness-file-pairs.json` freezes independently captured, identical SHA-256 digests
from both pins for eight direct-file factories: wrapping, wide characters, tabs, deletion-only,
multiple hunks, expandable context, scrolling, and watch startup. The native factory test compares
every before/after byte digest and checks the watch-only Git initialization. This verifies those
fixture outputs, not the containing harness's remaining factories or terminal differential gate.
Live layout and note PTY cases use the checked factories for wrapping, wide characters, tabs,
multiple hunks, context expansion, and scrolling, rather than maintaining separate fixture text.
Watch-driver coverage also exercises the source's detached linked worktree with `watched.ts`,
alongside the existing branch-attached variant. Both use a real common Git directory and observer
events; direct-file atomic-save coverage now includes the source fixture's initially empty Git
repository. These changes preserve the original refresh deadline and do not replace PTY parity.
Snapshot lookup helpers have separate dual-pin oracle cases in
`oracles/pty-harness-snapshot-helpers.json`, including missing/empty needles and astral Unicode.
The source's rightmost match is a UTF-16 string offset, not a terminal-cell coordinate; the native
helper preserves that contract without changing the renderer's display-width calculations.

`oracles/pty-file-views.json` records thirteen baseline passes, the stable suite's initial
menu timeout, isolated retry, and full thirteen-case passing retry. Native tests exercise compiled
Rust examples against pinned input blobs, including three retained presentations in a mixed-file
scroll stream, Markdown note binding/fallback, inline edits and emoji deletion, menu dispatch,
and interactive-mode input ownership. Mode exit clears its stale status hint. Busy native
connections defer keyboard input in order rather than dropping a key and retiring the mode;
the queue retains ownership identity across retries. File-view notes
include agent-context annotations and exclude virtual thread drafts. Example titles retain their
upstream visible text; no JSX or JavaScript runtime is used. Full cell-buffer differential parity
remains a separate release gate.

`oracles/pty-extensions.json` records seventeen tests and 69 assertions passing at each pin,
with all seventeen native PTY translations. Coverage includes repository trust acceptance,
dismissal and persistent denial; native Ctrl-C shutdown; pane command/menu routing, replacement
slots and edge resize geometry; compact confirmation dialogs; bundled note navigation, snapshot
export, triage and Vim; line-highlight refresh; measured line reveal and transient notifications.
Unix extension children use their own process groups so the reviewer can deliver ordered shutdown
instead of foreground SIGINT killing them first. Extension reveal uses live measured line rows,
not the hunk anchor. Notifications retain the source `ext` surface and expire without a persistent
status copy. These assertions do not waive Windows lifecycle or full terminal-cell parity gates.
The native trust fixture commits its compiled extension and manifest before changing the two
source files, matching the source harness's tracked-extension baseline. Its alpha/beta contents
now match that factory rather than borrowing the distinct layout fixture. A shared repository
factory verifies committed pre-change bytes, retained prepared entries, exact changed paths,
source author/message metadata, and owned temporary-directory cleanup. No compiled fixture is
added to Workdeck's own tracked tree; the binary lives only in a disposable test repository.

Lifecycle capture now retains a separate stderr pipe for failures after terminal revocation and
continues draining output while revoking. A zero-byte terminal write is classified as disconnect
alongside EIO and broken pipes; it is not swallowed for unrelated I/O. Five consecutive native
lifecycle suite runs passed after this correction.
Lifecycle fixtures now also run sequentially, matching the source suite: Darwin terminal revocation
can otherwise hold terminal subsystem locks across independent fixtures. Original exit deadlines
remain unchanged.

Git/Jujutsu source collection owns a dedicated Unix subprocess group. Overflow cleanup terminates
descendants as well as the direct child so inherited output pipes cannot keep reader joins blocked.
The diagnostic-overflow regressions retain their original Git two-second and Jujutsu five-second
deadlines with ten-second descendant
sleeps; a separate test checks cleanup after the parent exits on TERM and verifies an unrelated
process remains alive. This does not assert Windows process-tree cleanup parity.
