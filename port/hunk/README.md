# Hunk semantic-port ledger

## Local verification checkpoint: fd0e52ce

`cargo xtask verify` passed at clean commit `fd0e52ce`, including workspace
tests, Clippy, the release build, and the large-repository smoke test. This is
local verification, not release or parity certification: 313 ledger records
remain unmapped, the recorded upstream queue still contains 11 commits, and the
historical trailer, benchmark, and cross-platform release gates remain open.

## Live context-gap mouse routing

The live shell projects expandable-gap hits from its final row layout into the
visible Ratatui viewport. Gap rows outside that viewport, zero-size renders, and
alternate file views do not publish raw-gap targets. Draft-note insertion shifts
the logical gap rows with the rest of the row metadata. Hits carry a semantic file
key, gap slot, and document generation; stale document frames cannot toggle a
replacement review before it is painted.

Mouse release uses the same toggle/cursor path as the keyboard. A press on a gap
does not begin text selection, and an active copy drag retains precedence. Saved
restore points carry their own cursor file key, separate from the gap's owner, so
expanding another file's gap and reordering files still restores the original
cursor file on collapse.

Native tests cover expansion/collapse, stale frames, viewport clipping, zero-size
rendering, and cross-file restoration after reordering. A real CLI PTY test sends
SGR mouse presses/releases in both stack and split layouts and checks the hidden
source line appears and disappears. This integration currently exposes gaps with
an available source snapshot; asynchronous source-fetch capability and the full
DiffPane/controller port remain unfinished and unmapped. No source interval gains
coverage from this partial controller integration.

Gap rows now use the ported metadata painter: materialized source gaps show the
interactive chevron, while gaps without a source snapshot use the noninteractive
dotted label and publish no mouse target. The painter supplies the selected or
dimmed rail, themed foreground/background, width clipping, and panel padding.
A live row-builder regression checks labels, absent hit metadata, selected and
unselected colors, and exact widths at 0, 1, 2, 12, and 80 columns. This does not
implement lazy source fetching or the metadata row's hover/add-note controls.

## Live gap retirement on reload

The native TUI now uses the shared semantic source-identity retirement policy when
committing a replacement changeset. Removed files and changed source identities
retire both open gaps and saved cursor restore points. Reintroducing a removed
file cannot resurrect its previous expansion. An unchanged source identity keeps
its expansion, including when only the runtime file identifier changes.

Surviving restore points are relocated by semantic file key when files reorder;
their old numeric indexes cannot target another file. Current-cursor lookup also
requires the selected file and hunk, and falls back to that hunk's first cursor
when no prior cursor remains. Collapsing a gap restores its saved anchor only if
the current line disappeared; a cursor moved outside the gap stays where it is.
Native regressions reproduced the previous wrong-file jump and unconditional
restore. These cases are source-derived runtime tests, not additional frozen
upstream terminal-frame comparisons. With no expansion state, reload skips this
reconciliation work.

The review core preserves an expanded source-line selection across a reload only
when its semantic file key and present source identity survive, the owner hunk
still exists, and the selected side still contains that line. This avoids losing
the exact cursor line when an unchanged expanded file moves in the stream.
Changed identities, changed keys, missing/short source snapshots, and removed
owner hunks do not retain that source selection. Ordinary changed-line selection
continues to use its existing hunk validation path.

The changed/removed-source regression failed against the preceding live TUI and
passes with this integration. The unchanged-source case verifies both retained
state and rendered source rows. Its fixture refreshes derived identities after
attaching full source text, matching provider preparation. The shared semantic
query also tests all nine absent/present identity pairs and duplicate-key behavior.

`oracles/terminal-review-gap-reload-execution.json` records actual execution of the
two related upstream soft-reload tests from both pinned checkouts: two tests and
22 assertions pass per pin. These are limited execution receipts, not full controller
or terminal-frame fixtures. `useTerminalReview.ts` and its full test file remain
unmapped; asynchronous source loading and the rest of their contracts are not
certified by this fix.

## Commit provenance validation

`cargo xtask port history` checks annotated Workdeck commits after the `dc2ac39`
base without changing Git history or repository state. Git extracts the actual
trailer block, preserving repeated values and excluding prose mentions. Each
`Hunk-Port:` identifier must exist in the ledger **at that commit**, not merely
in today's ledger: subsequent interval splitting must not invalidate historical
references. Each annotated port commit must also identify upstream provenance;
`Hunk-Upstream:` values must be full commit hashes reachable from preserved Hunk
branches, tags, or the two source anchors.

Strict audit invokes this check after its source-coverage gate. The CI checkout
fetches complete Workdeck history for validation; release preflight already did so.
This validates references, not the semantic contents of a port commit, and does not
yet identify all missing trailers on entirely unannotated commits.

At `1594ea43`, the check examined 455 annotated commits among 464 Workdeck commits
and found ten unresolved historical provenance issues: three missing upstream
trailers and seven `baseline` shorthand values instead of explicit hashes. The
command reports every affected commit. History has not been rewritten, no errors
are waived, and this is an additional failing release gate. Documentation migration
has reduced the current unmapped count to 313; the upstream queue remains eleven.

## Executable test-anchor validation

The mapped-test evidence gate parses Rust syntax instead of searching for test-looking
lines. Comments and string literals cannot satisfy it. Every explicit Rust test anchor
must resolve to an attributed test function or an inline module containing tests;
qualified anchors follow the named module path, while an unqualified function name
can identify a nested test. One valid evidence entry cannot hide another stale anchor.
Unanchored Rust evidence still supports attributed tests and property-test macros.
Parsed source is cached only for the duration of one audit invocation.

This check found and corrected two stale CLI module names and two descriptive labels
for existing scrollbar/file-presentation tests. No source intervals or dispositions
changed. At the `b559b5c7` checkpoint, strict audit still failed with 314 unmapped
records and 11 pending upstream commits. Syntax validation is not execution evidence, per-source assertion coverage,
or proof of semantic parity; the actual tests and remaining release gates are still
required.

## Native planning-profile workload

`cargo xtask benchmark large-stream-profile` runs the complete pinned
`benchmarks/large-stream-profile.ts` workload without a terminal renderer: section
geometry, split-row construction, then review-plan construction over 180 files of
120 lines each. Fixture construction stays outside the timers; the theme is the
pinned `midnight` alias, headers are enabled, and no agent notes are supplied.
All seven source metric names, ordering and two-decimal timing formatting are retained.
The normal subprocess runner also accepts `--script large-stream-profile.ts`.

`oracles/benchmark-large-stream-profile.json` retains direct outputs from both pinned
source scripts. The executable Rust test runs all three native stages and checks both
10,260-row totals and the fixture counts against each oracle. Oracle timing values were
captured under concurrent verification load and are not accepted benchmark comparisons.
Implementing this workload does not establish the product's overall performance gate.

## Local verification checkpoint `c453574b`

On 2026-09-08, `cargo xtask verify` passed the theme/skill/architecture/history checks,
full workspace tests, workspace Clippy, release build and 300-change repository smoke.
This rerun includes the deferred-command scheduling regression. A preceding verify run
failed the line-highlighter PTY contract; a deterministic held-connection regression
reproduced and fixed the queue starvation before this passing run. `cargo deny check`
also passed during this work, with duplicate/dependency-policy warnings.

Strict `port audit` still fails: 1,257 baseline files, 1,326 interval records, 315 unmapped
records and 11 pending upstream commits. The upstream count reflects the preserved fetched
refs, not a final release-time fetch. Remote native CI, signed archives, full benchmark
parity and all remaining source work are not certified by this local checkpoint. See
`benchmarks/interaction-paired-peak-c453574b.json` for the fresh, limited interaction/peak-RSS
comparison; scrolling remains outside budget.

## Explicit native CI hosts

CI and release build matrices name all five native targets: Linux x64/arm64, macOS
x64/arm64 and Windows x64. Runner labels use `ubuntu-24.04`, `ubuntu-24.04-arm`,
`macos-15-intel`, `macos-15` and `windows-2025`, following the
[GitHub-hosted runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
checked on 2026-09-08. `cargo xtask ci-host <target>` requires the rustc host triple
and xtask's compiled OS/architecture to match the matrix target. Local tests reject
missing/duplicate/wrong host reports and check both complete YAML matrices.

Only the local macOS arm64 host check has been executed here. The five remote jobs,
installer smoke coverage and signed release verification remain unexecuted; matrix
configuration is not native CI evidence. No workflow was pushed or triggered.

## Native CI change detection (partial tooling port)

`cargo xtask ci-changes <base-revision> <head-revision>` implements the pinned CI
docs/assets classifier, all-zero-base empty-tree comparison, shallow-checkout object
fetching from `origin`, and rename-disabled comparison. It appends `code_changed=true`
or `false` to a nonempty `GITHUB_OUTPUT` and preserves existing output-file contents.
Markdown suffix matching is case-sensitive; `docs/`, `assets/`, and root `LICENSE`
are exempt. Git's original quoted filename output is preserved for classification.

Frozen outputs from both source pins are in `oracles/ci-code-changes.json`; the ignored
Rust capture test executes each script directly from its preserved Git blob in disposable
fixture repositories. Regular Rust tests cover the decisions, append behavior, shallow
fetch, and failures without requiring Bash or an upstream runtime.

The source ledger record remains unmapped: workflow integration and the full error/input
contract are not yet reconciled. In particular, the Rust command rejects option-like or
control-character revisions and extra arguments, and fails closed on Git diff failures.
The source process-substitution loop can instead report no code changes after a failed diff.
These differences are explicit unresolved parity work, not a completed-file waiver.

## Deferred native command scheduling

Background highlighting and file-view work share an extension's ordered subprocess
connection. Commands queued while that connection is busy must be retried on later
host polls even when no command/event completion exists to wake the queue. The host
now retries deferred queues each poll, retaining the existing FIFO and busy checks.
A controlled PTY regression holds a real highlight request, queues F8, releases the
connection, and requires its refresh-control result without any further input. It
reproduced a permanent pending-command failure before the retry was added. The hold
has a bounded deadline and the test checks that it did not expire before queuing.

## Immutable native review documents

`ReviewState` owns its changeset through `Arc<Changeset>`. Its `changeset()` accessor remains
read-only, and `changeset_snapshot()` retains that exact immutable document. State clones share
the document; selection, layout and comments remain independently owned state. Reload installs
a new allocation. A fresh replacement state is distinguishable even if its generation and file
IDs equal an older state's values. A retained `Arc` prevents allocation-address reuse while a
consumer uses its identity. Caller copy-on-write edits cannot mutate the state-owned document.

The public `ReviewSnapshot` still contains an owned `Changeset`; its serialized schema is not
changed. A one-entry TUI content-height cache now retains this identity after a completed plain,
unwrapped split render. Wheel handling reuses the exact height only when document identity,
layout, width, filtering, file/hunk spacing, header/pager settings and registry generation match.
Notes, wrapping, expanded gaps, agent line highlighting and active extensions bypass reuse;
complex render paths clear the retained entry. The same entry retains style-free file-section
positions, allowing subsequent eligible renders to skip the geometry prepass used for viewport
clamping and highlight prefetch. Full painted-row construction still runs. A Unicode frame test
compares every Ratatui cell against a forced rebuild after theme changes and resizing, while
holding scrollbar interaction history constant; it also checks section allocation reuse.
The cache stores no painted rows and does not by itself prove a performance gate. Broader
geometry reuse must account for all dynamic content; generation alone is insufficient.

File-presentation menu projection also retains the immutable document rather than cloning
all file bodies. Selected-file matching, draft availability, and bulk targets use references
into that snapshot after releasing the review-state lock. Bulk targets still include matching
files hidden by the current filter; the existing menu/bulk-action regression exercises this,
selection persistence, and reload removal.

The live Files sidebar builds lightweight entries directly from filtered references and uses
that same entry list for selection reveal and rendering. Public slice-based entry/render APIs
remain unchanged. A differential test compares full cell buffers and hit maps with the owned,
filtered public-renderer path over Unicode paths, both sidebar modes, narrow/zero widths,
empty filters and out-of-range scrolling. File bodies are no longer copied for sidebar rows.

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
