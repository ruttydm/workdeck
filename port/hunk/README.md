# Hunk semantic-port ledger

## Narrow wrap-toggle geometry: corrected and mapped

The three source wrap-toggle tests (bytes 36,159–39,143, lines 1177–1278 of
`AppHost.interactions.test.tsx`) now map to a Rust matrix covering regular and
pager wrapping plus repeated on/off/on toggles. Both pinned source runs pass
three tests and fourteen assertions. The [failure-to-fix evidence](oracles/app-host-wrap-toggle.json)
retains the original native suffix failure (`e';` instead of `age';`).

The fix separates terminal width for automatic layout from pane-minus-two width
for row geometry, wrapping, notes and extension file views. Tests cover terminal
widths 119–122 and sidebar-visible widths 160, 180 and 220. The cell-sampling test
helper now restricts character lookup to the matched text, so split-row tests
cannot accidentally inspect the old side when targeting new-side content.

Pinned `DiffPane.tsx:2669–2680` supplies a top border and one padding row before
the pinned header. Native rendering now matches that geometry, including header
hits, copy/reveal offsets, pager/menu variants and rejection of bottom-padding
note hover. The source-absent border title is removed. Pinned `App.tsx:424–437`
also reserves independent status/toast rows only when needed. Replacing the
native unconditional empty footer resolves eight viewport/navigation failures
exposed by the padding correction. Root-render tests retain startup-notice
precedence within the status row while checking a separate extension-toast row.

The combined implementation passes 1,156 TUI library tests (7.87s), formatting
and warnings-denied TUI all-target Clippy. A stronger pane comparison then passes
in 0.79s: every character in rows 1–23 at 102×24, including blanks, matches both
[frozen Hunk captures](oracles/app-host-wrap-frames.json) on both wrapped frames.
The menu/title row and style/color data are outside that character-pane check.
This maps only the three complete source tests, not the larger App/DiffPane
implementation, all terminal sizes, or the complete product. The rest of their
source intervals and the release/performance gates remain open.

## Theme event preview leak: corrected and mapped

The main-only theme-event interaction test exposed an integration gap: production
event facts used the rendered preview ID instead of the committed preference.
The new regression failed with dimmed versus default after preview; switching
the projection to the committed identity makes preview, Escape, and acceptance
checks pass (one test, 0.78s). Formatting and TUI Clippy pass.
See [source run and native failure/correction](oracles/app-host-theme-events-partial.json).
Stable has no matching test; its zero-match run is not counted as a pass.
The runtime fix passes all 1,151 TUI library tests (51.70s). A subsequently added
publisher-level test passes (0.82s): preview, Escape, removal/restoration of the
custom catalog emit no theme event; acceptance emits exactly one expected ID.
This observes the production publication hook, not an extension subprocess.
Catalog inputs are applied through the controller rather than a complete
bootstrap reload. Formatting and TUI Clippy also pass after that added test.
The real CLI PTY test `theme_subscriber_receives_acceptance_but_not_preview_or_escape`
now passes (7.93s): a compiled Rust extension receives exactly one theme change
for acceptance and none for preview/Escape. It records actual protocol events
to a disposable JSONL file and explicitly discards preference changes on exit.
The first run passed event assertions but failed cleanup at the expected save
prompt; the corrected rerun passes through process exit. Formatting passes.
The catalog regression now uses `session_commit_dynamic_reload` with resolved
host options instead of setting controller inputs directly. Preview, Escape,
catalog removal/restoration, and acceptance checks pass. Together with the
subscriber PTY, this maps source bytes 33,267–36,159 (main only). These are
separate integration tests, not one full catalog-plus-subprocess scenario.
All 1,152 current TUI library tests pass (59.90s). TUI/example all-target Clippy,
CLI PTY-test Clippy, formatting, and diff checks pass. Strict audit remains
incomplete: 268 unmapped intervals, 1,384 records, and 11 cached upstream commits
pending. The capture filename retains “partial” to preserve its history.

## Theme reopening and Escape cancellation

Bytes 30,255–33,267 map to two 240×24 interaction tests: accepting dracula-soft
and reopening on it, and cancelling a high-contrast preview to restore the
original theme. Both pinned runs pass six assertions; native additionally
checks selected rows and applied theme identities. Formatting and TUI Clippy
pass. See [capture and limits](oracles/app-host-theme-reopen.json).
The ledger has 1,383 records, 268 unmapped intervals, and 11 cached upstream
commits pending. Full parity remains unproven.

## Theme list wheel scrolling

Bytes 29,111–30,255 map to the 240×24 theme-wheel interaction test. Both pins
pass their two initial assertions. Their post-scroll predicate is polled but
not asserted; native explicitly verifies catalog movement and unchanged preview
identity. This stronger native check is not a frozen source frame comparison.
See [capture and scope](oracles/app-host-theme-wheel.json). Native test,
formatting, and TUI Clippy pass. The ledger has 1,382 records, 268 unmapped
intervals, and 11 cached upstream commits pending; full parity remains unproven.

## Debounced theme hover and click acceptance

Bytes 27,051–29,111 map to the 240×24 rapid-hover interaction regression.
Two rendered theme rows receive mouse moves before a frame; the initial theme
remains selected until the production timer advances, then the last hovered
theme previews and a click accepts it. Both pinned source tests and the native
test pass; formatting and TUI Clippy pass. Native uses an explicit 250ms timer
advance rather than source sleep. See [capture and limits](oracles/app-host-theme-hover.json).
The ledger has 1,381 records, 268 unmapped intervals, and 11 cached upstream
commits pending. Complete parity remains unproven.

## Theme selector keyboard acceptance

Bytes 25,426–27,051 map to the 240×24 `t/j/k/j/Enter` interaction test.
Both pinned tests pass six assertions; the Rust test additionally checks each
highlighted row and the accepted theme identity. Formatting and TUI Clippy pass.
See [capture and scope](oracles/app-host-theme-jk.json). Mouse preview timing,
all themes, and full cell-buffer parity are not covered by this mapping.
The ledger contains 1,380 records, 268 unmapped intervals, and 11 cached
upstream commits pending. Full completion remains unproven.

## Startup notice and menu summary

Bytes 22,381–23,360 map to the configured deprecation-notice and menu-summary
interaction tests. The menu previously showed only the changeset title; it now
appends file count, additions, and deletions from the entire changeset, following
pinned `App.tsx` lines 1162–1172. Sidebar filtering does not change those totals.
Both pinned oracle runs and the native rendered test pass. See
[capture and scope](oracles/app-host-startup-summary.json).
All 1,146 TUI library tests pass in 53.22s; formatting and TUI Clippy pass.
Strict audit still fails on 268 unmapped intervals; 11 cached upstream commits
remain pending. Complete cell geometry and all truncation widths are not proven.

## Corrected hidden-menu geometry regression

The regression initially found an unconditional top `Review` border even when
the menu bar was hidden. The renderer now omits that border and uses shared
reserved-row and content-origin calculations for scrolling, note actions,
copy selection, file/gap targets, reload positioning, and scrollbar geometry.
Both toggle-hidden and configured-hidden cases now pass, including F10 access.
See [source captures, initial failure, and corrected result](oracles/app-host-hidden-menu.json).
Bytes 23,360–25,426 map to these two translated tests. All 1,145 TUI library
tests pass in 78.25s; formatting and TUI Clippy pass. This is not full terminal
cell parity or cross-platform verification. Splitting the remaining prefix
leaves 269 unmapped intervals across 1,379 records; 11 cached upstream commits
remain pending, and strict completion is still unproven.

## Notes, numbers, and metadata shortcuts

Bytes 20,987–22,381 map to the 240×24 single-file `a/a/l/m` interaction test.
Both pinned source runs pass all seven assertions; the native test also verifies
the line-number and hunk-header settings changed. Formatting and TUI Clippy pass.
See [oracle capture and scope](oracles/app-host-view-shortcuts.json).
Splitting the earlier unmapped prefix around this tested interval produces
1,377 records and 268 remaining unmapped intervals; it does not increase missing
source bytes. The file remains incomplete, with 11 cached upstream commits pending.

## Regular and pager quit keys

Bytes 119,471–122,597 map to the final three AppHost interaction tests:
pager quit after a wrapping change, regular/pager `q`, and non-quitting Escape.
The Rust matrix preserves all five source scenarios and terminal sizes. Both
pinned oracle runs pass three tests and six assertions; the native matrix passes.
See [capture and limits](oracles/app-host-quit-keys.json). Exit requests are
verified, not actual terminal process teardown or complete cell-buffer parity.

Validation: `cargo test -p workdeck-tui --lib` passes all 1,143 tests in 85.51s;
workspace formatting and TUI all-target Clippy pass. Strict audit still fails:
267 unmapped intervals, 1,375 records, 1,257 baseline files, and 11 cached
upstream commits pending. The earlier unmapped prefix of this source test file
remains incomplete; finishing its tail does not complete the file.

## Transient extension preference policy

Bytes 118,487–119,471 map to the 180×24 transient session regression.
Native extension registration metadata passes through the production policy
resolver, then the reviewer changes wrapping and quits without prompting or
writing configuration. Both pinned source tests and the Rust test pass;
formatting and TUI Clippy pass. This injected-registration test does not prove
subprocess lifecycle or complete CLI bootstrap behavior. See
[capture and scope](oracles/app-host-quit-transient.json).
The ledger has 1,375 records, 268 unmapped intervals, 1,257 baseline files,
and 11 cached upstream commits pending.

## Disabled preference prompt

Bytes 117,209–118,487 map to the 240×24 configured-opt-out regression:
change theme, quit immediately, show no save prompt, and create no configuration.
Both pinned oracle tests and the Rust test pass; formatting and TUI Clippy pass.
See [capture and verification limits](oracles/app-host-quit-prompt-disabled.json).
The adjacent transient-extension test is mapped in the subsequent entry above.
The ledger now has 1,374 records, 268 unmapped intervals, 1,257 baseline files,
and 11 cached upstream commits pending. Full parity remains unverified.

## Quit-dialog mouse actions and persistent opt-out

Bytes 114,400–117,209 map to the 240×24 rendered mouse-action regression:
cancel without exiting or writing configuration, reopen, then save the theme
and request one delayed exit. Both pinned oracle tests and the Rust test pass;
formatting and TUI Clippy pass. See [capture and input/timer limits](oracles/app-host-quit-mouse.json).

The preceding bytes 112,495–114,400 map to the separately tested keyboard
“never ask” action, which persists the prompt opt-out and schedules one exit.
See [opt-out evidence](oracles/app-host-quit-never-ask.json).
Neither mapping claims terminal teardown or full cell-buffer parity.
The ledger has 1,373 records across 1,257 baseline files; 268 intervals and
11 cached upstream commits remain incomplete.

## Quit-time theme persistence

Bytes 110,611–112,495 map to the 240×24 save-and-quit regression. Both pins
pass; Rust verifies the written theme and delayed, single-use quit request.
Native test, formatting and TUI Clippy pass. See [capture and timer scope](oracles/app-host-quit-save.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,371 records,
1,257 baseline files and 11 cached upstream commits pending.

## Quit prompt and discard after theme change

Bytes 108,360–110,611 map to the 240×24 theme-change prompt/discard regression.
Both pins pass nine assertions; native verifies changed-theme-only rows, deferred
quit and discard without creating configuration. Test, formatting and TUI Clippy
pass. See [capture and scope](oracles/app-host-quit-prompt.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,370 records,
1,257 baseline files and 11 cached upstream commits pending.

## Sidebar click after scrolling

Bytes 106,992–108,360 map to rendered sidebar-row targeting after eight Down
events at 220×10. Both pinned tests pass; native follows main's rendered-row
selection rather than stable's fixed y coordinate. Native test, formatting and
TUI Clippy pass. See [captures and distinction](oracles/app-host-sidebar-click.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,369 records,
1,257 baseline files and 11 cached upstream commits pending.

## Down-arrow selection publication

Bytes 105,607–106,992 map to the down-arrow snapshot regression at 220×12.
Both source pins pass; Rust reaches second.ts hunk 1 within the source's 50-event
limit. Wheel and page tests pass after fixture refactoring; formatting and TUI
Clippy pass. See [capture and limits](oracles/app-host-down-selection.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,368 records,
1,257 baseline files and 11 cached upstream commits pending.

## Page-key selection publication

Bytes 103,482–105,607 map to PageDown/PageUp selected-file publication at 220×12.
Both pins and Rust pass; the wheel test also passes after sharing its fixture
builder. Formatting and TUI Clippy pass. See [capture and limits](oracles/app-host-page-selection.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,367 records,
1,257 baseline files and 11 cached upstream commits pending.

## Wheel viewport selection integration

Wheel scrolling now updates the selected file/hunk from rendered viewport-center
geometry and publishes selection without scrolling back. The source regression
at bytes 102,092–103,482 passes through the native snapshot publisher.
All 1,133 TUI tests pass (74.42 seconds), formatting and TUI Clippy pass.
See [implementation and limits](wheel-selection.md) and [both pinned oracles](oracles/app-host-wheel-selection.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,366 records,
1,257 baseline files and 11 cached upstream commits pending. Performance remains
a separate unpassed gate.

## Forward and backward cross-file hunk sequence

Bytes 100,094–101,173 and 101,173–102,092 map to exact 18-next/one-next/two-previous
navigation at 120×16. Both source pins pass five assertions; the Rust sequence
passes with the same first-header selector and backward target checks.
Formatting and TUI Clippy pass. See [capture and limits](oracles/app-host-cross-file-sequence.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,365 records,
1,257 baseline files and 11 cached upstream commits pending.

## File shortcuts and published selection

Bytes 98,079–100,094 map to period/comma file navigation and filter focus.
Both source pins pass six assertions. The Rust test uses the production snapshot
publisher and an in-process host, checking selected file IDs and hunk index as
well as visible destination/filter text. Test, formatting and TUI Clippy pass.
See [capture and transport limits](oracles/app-host-file-shortcuts.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,363 records,
1,257 baseline files and 11 cached upstream commits pending.

## Cross-file destination header after scrolling

Bytes 97,022–98,079 map to ten Down events followed by next-hunk navigation
at 220×10. Both source pins and Rust pass the destination visibility/old header
count assertions; Rust additionally checks the selected file. Formatting and
TUI Clippy pass. See [capture and limits](oracles/app-host-destination-header.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,362 records,
1,257 baseline files and 11 cached upstream commits pending.

## Pager and responsive sidebar toggles

Bytes 95,078–96,109 and 96,109–97,022 map to sidebar overrides in pager mode
(220×24) and responsive review (159×24). Both source pins pass eight assertions;
the native matrix passes, along with formatting and TUI Clippy.
See [capture and limits](oracles/app-host-sidebar-modes.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,361 records,
1,257 baseline files and 11 cached upstream commits pending.

## AppHost sidebar visibility toggle

Bytes 94,201–95,078 map to the 240×24 sidebar off/on regression, checking
rendered alpha.ts counts of two, one, then two. Both pinned source tests and
the native translation pass, as do formatting and TUI Clippy.
See [capture and scope](oracles/app-host-sidebar-toggle.json).
Strict audit remains incomplete: 268 unmapped intervals, 1,359 records,
1,257 baseline files and 11 cached upstream commits pending.

## Draft focus lifecycle correction

The native draft now releases input focus on outside clicks without losing its
body. Clicking inside restores editing; paste, caret, menu suppression and inline
presentation follow focus rather than draft existence. The source blur regression
at bytes 93,028–94,201 now passes, with additional blur/refocus checks.
All 1,127 TUI tests pass (86.22 seconds), formatting and TUI Clippy pass.
See [implementation and verification](draft-focus.md) and [both pinned oracles](oracles/app-host-draft-blur.json).
Strict parity remains incomplete: 268 unmapped intervals, 1,358 records,
1,257 baseline files and 11 cached upstream commits pending.

## AppHost tmux CSI-u draft saving

Bytes 92,091–93,028 map to Ctrl-S saving through a native PTY at 240×24.
Both pinned source tests pass. Rust sends actual `ESC[115;5u` bytes into the
shipped CLI terminal-input path and verifies saved content with the draft gone.
The native test passes (3.30 seconds); see [capture and transport scope](oracles/app-host-draft-csi-u.json).
No tmux process or Windows transport is covered by this macOS run.
Strict audit still fails with 268 unmapped intervals across 1,357 records and
1,257 files; 11 cached upstream commits remain pending.

## AppHost large draft input burst

Bytes 91,167–92,091 map to the 160×40 synchronous input-burst regression.
Both pinned source tests and the Rust translation pass. Native coverage also
asserts the complete stored body, not only visible prefix/suffix text.
Formatting and TUI Clippy pass; see [capture and limits](oracles/app-host-draft-large-burst.json).
Strict audit remains incomplete with 268 unmapped intervals in 1,356 records,
1,257 baseline files, and 11 cached upstream commits pending.

## AppHost CJK draft wrapping and capture correction

Bytes 90,177–91,167 map to chunked CJK draft input at 160×40. Both pinned
source tests pass. The Rust test initially exposed a capture-helper error:
covered wide-character cells were concatenated as visible text. The helper now
advances by display width, matching Ratatui's output semantics. Production
rendering was unchanged. All 1,125 TUI tests pass (51.28 seconds), as does TUI
Clippy. See [capture, correction and validation](oracles/app-host-draft-cjk.json).
There remain 268 unmapped intervals across 1,355 records and 1,257 baseline
files, plus 11 cached upstream commits pending. This is not complete parity.

## AppHost draft focus and shortcut suppression

Bytes 89,162–90,177 map to draft input ownership at 240×24. Both pinned source
tests pass five assertions; native coverage additionally checks the exact draft
body is `s` and the sidebar option is unchanged. The strengthened test, formatting
and TUI Clippy pass. See [captures and scope](oracles/app-host-draft-focus.json).
Strict audit still fails: 268 unmapped intervals in 1,354 records across the
unchanged 1,257 baseline files; 11 cached upstream commits remain pending.

## AppHost burst movement and draft anchoring

Bytes 87,481–89,162 map to the rapid eight-Down-then-comment regression.
Both unchanged pinned tests pass four assertions. The Rust test verifies L9,
unchanged source-row position, immediately following draft placement, and movement
of subsequent code at 120×26. Formatting and TUI Clippy pass.
See [capture and synchronous-event scope](oracles/app-host-draft-burst.json).
This leaves 268 unmapped intervals in 1,353 records across 1,257 files,
with 11 cached upstream commits pending; strict completion remains unproven.

## AppHost transparent-background rendering

Bytes 84,950–86,341 and 86,341–87,481 map to menu/help opacity and diff-row
tint regressions. Both pinned sources pass seven assertions; the native test
passes text and Ratatui cell-background checks at 220×24 and 220×60.
Formatting and TUI all-target Clippy pass. See [captures and limits](oracles/app-host-transparent.json).
Strict audit remains incomplete: 1,352 records, 268 unmapped intervals,
1,257 baseline files and 11 cached upstream commits pending.

## AppHost top-level menu wrapping

Bytes 83,786–84,950 now map to the 220×24 F10/Left/Right menu regression.
Both pinned source tests pass six assertions; the native translated test passes
and formatting passes. See [capture and scope](oracles/app-host-menu-wrap.json).
Strict audit still fails with 268 unmapped intervals across 1,350 records and
1,257 baseline files. Eleven cached upstream commits remain pending.

## Corrected annotation fixtures and deep-note navigation

Eight translated fixtures now explicitly assert their internal annotation ranges.
Earlier range claims are superseded by the [fixture correction](annotation-fixture-correction.md).
Correct ranges exposed a filtered-navigation bug: relative navigation must walk
visible files while preserving a hidden document selection. The corrected runtime
passes all 1,120 TUI library tests (49.80 seconds), formatting and TUI Clippy.

The reopened filter regression is now verified again. Bytes 82,720–83,786 map
to the [deep-note navigation regression](oracles/app-host-deep-note.json), executed
against both pinned sources and the native controller. No whole-file or live
transport parity is implied. There are 1,349 ledger records and 268 unmapped
intervals across the unchanged 1,257 files; 11 cached upstream commits remain.

## AppHost filtered session navigation (partial interaction corpus)

Bytes 81,253–82,720 map to the session comment-navigation filter regression.
At 240×24, the beta query remains active and the same code stays visible after
the no-annotated-hunks error. Both unchanged source tests pass eight assertions;
native test, formatting and TUI all-target Clippy pass. See
[capture and controller-only scope](oracles/app-host-session-filter.json).
This is not live daemon transport or general filtered-annotation-set parity.

At this earlier checkpoint, strict audit validated partitions/evidence, then failed with 1,257 files,
1,348 records, 390 translated-test records and 268 unmapped intervals. Eleven
cached upstream commits remain pending; complete parity is not established.

## AppHost filter selection contract (partial interaction corpus)

Bytes 80,168–81,253 map to the executed filter/query regression at 240×24.
Both source runs pass six text assertions; native test, formatting and TUI
all-target Clippy pass. Crucially, the old test title says "reselects" but its
assertions do not check selection. Both pinned core selectors preserve a selected
file hidden by a filter. Native coverage now explicitly preserves alpha selection
while displaying beta, and retains the query after Tab. A provisional contrary
runtime change was removed before commit. See [evidence and discrepancy](oracles/app-host-filter-selection.json).

At this earlier filter-selection checkpoint, strict audit validated partitions/evidence, then failed with 1,257 files,
1,347 records, 389 translated-test records and 268 unmapped intervals. Eleven
cached upstream commits remain pending; whole-file and release parity are incomplete.

## AppHost pager edges and empty filtering (partial interaction corpus)

Bytes 77,584–79,396 and 79,396–80,168 map to executed pager `G/g` and
Tab/`zzz` filter tests. Pager checks use 220×12 without menu chrome; the 240×24
filter case checks the label, typed query and empty-match message. Both pinned
source runs pass six assertions, and both native tests, formatting and TUI
all-target Clippy pass. See [captures and limits](oracles/app-host-pager-filter.json).
Selection reconciliation after filtering and complete cell frames remain separate
requirements, not implied by this empty-result test.

At this earlier pager/filter checkpoint, strict audit validated partitions/evidence, then failed with 1,257 files,
1,346 records, 388 translated-test records and 268 unmapped intervals. Eleven
cached upstream commits remain pending; complete parity is not established.

## AppHost content-edge authority (partial interaction corpus)

Bytes 76,716–77,584 map to the executed `]` then Shift+`g` regression, retaining
the bottom short-file hunk across repeated paints at 120×16. The unchanged
main-baseline test passes; this test is absent from stable, whose zero-match
exit is recorded rather than called a pass. Native test, formatting and TUI
all-target Clippy pass. See [capture and timing limits](oracles/app-host-content-edge-authority.json).
Native updates are synchronous; this does not claim coverage of all asynchronous
extension or source-loading races.

At this earlier content-edge checkpoint, strict audit validated partitions/evidence, then failed with 1,257 files,
1,344 records, 386 translated-test records and 268 unmapped intervals. Eleven
cached upstream commits remain pending; complete parity is not established.

## AppHost paging aliases and content edges (partial interaction corpus)

Bytes 72,872–74,945 and 74,945–76,716 map individually to the executed alias
and `G/g` edge-navigation regression. The source 50/120-line fixtures at 220×12
exercise `d`, `u`, `f`, Shift+Space and modified lowercase `g`. Both source tests
pass on both pins (seven assertions each run), and native test, formatting and
TUI all-target Clippy pass. See [capture and limits](oracles/app-host-paging-aliases.json).
Alias acceptance does not prove exact paging distance; typed key events do not
replace terminal escape-decoding or complete cell-frame verification.

At this earlier alias checkpoint, strict audit validated partitions/evidence, then failed with 1,257 files,
1,343 records, 385 translated-test records and 268 unmapped intervals. Eleven
cached upstream commits remain pending; the goal remains incomplete.

## Full TUI regression checkpoint

At clean commit `f4018ebee09c588e9d6d11e721253adf7f3a1ad4`, the complete
TUI library suite passed: 1,114 tests, zero failures, ignored or filtered tests,
53.32 seconds. Native atomic-file-save, detached-worktree and linked-worktree
watch tests completed successfully. See [verification record](verification-f4018ebe.json).
This checkpoint covers the recent interaction-test batch, not the entire workspace
or release matrix. It adds no source-ledger coverage; 268 intervals and eleven
cached upstream commits remain unfinished.

## AppHost Space/PageUp paging (partial interaction corpus)

Bytes 69,852–71,331 and 71,331–72,872 map individually to the native paging
regression. Both source fixtures use 50 numbered lines at 220×12. Their two
code-visible assertions pass on both pins; native checks also verify positive
scroll after Space and restored top after PageUp. These additional checks do not
prove an exact source paging distance. See [raw captures and limits](oracles/app-host-viewport-paging.json).
Native test, formatting and TUI all-target Clippy pass.

Current strict audit validates partitions/evidence, then fails with 1,257 files,
1,341 records, 383 translated-test records and 268 unmapped intervals. Eleven
cached upstream commits remain pending; release parity is not established.

## AppHost layout anchor (partial interaction corpus)

Bytes 67,995–69,852 map to the executed split/stack viewport-anchor test.
At 220×12, Down-key scrolling followed by `2` and `1` preserves the first
visible source line. The fixture includes the source helper's line-two annotation.
Both unchanged pinned source tests pass nine assertions. Native test, formatting
and TUI all-target Clippy pass; see [capture and limits](oracles/app-host-layout-anchor.json).
This does not establish whole-frame equivalence.

At this earlier layout-anchor checkpoint, strict audit validated partitions/evidence, then failed with 1,257 files,
1,339 records, 381 translated-test records and 268 unmapped intervals. Eleven
cached upstream commits remain pending; whole-file and release parity are incomplete.

## AppHost wrap anchor (partial interaction corpus)

Bytes 66,207–67,995 map to the executed wrap-toggle viewport regression. At
102×12, both `w` toggles retain the first visible added line after Down-key
scrolling. Both pinned source runs pass nine assertions. Inspection also corrected
helper-default annotations to line two in the wrap/arrow fixtures and alpha's
viewport-note fixture; beta keeps its explicit line-one override. All three
affected native tests pass together, with formatting and TUI all-target Clippy
clean. See [results and scope](oracles/app-host-wrap-anchor.json).

At this earlier wrap checkpoint, the ledger had 1,257 files, 1,338 records, 380 translated-test records and
268 unmapped intervals; eleven cached upstream commits remain pending. Strict
completion and whole-frame parity remain unproven.

## AppHost arrow scrolling (partial interaction corpus)

Four test bodies, bytes 60,795–66,207, map individually to executed native
regressions for review/pager Down-Up loops and pinned-header cursor-off/default
behavior. All four unchanged source tests pass on both pins (17 assertions each).
The two Rust tests pass, with formatting and TUI all-target Clippy clean.
See [raw source output and intervals](oracles/app-host-arrow-scroll.json) and the
[non-asserting wait investigation](pinned-header-keyboard-gap.md). A timed-out
source polling predicate is not promoted to a guarantee; complete cell-frame
equivalence remains a separate gate.

At this earlier arrow checkpoint, strict audit validated partitions/evidence and reported 1,257 files,
1,337 records, 379 translated-test records and 268 unmapped intervals, then
fails on unfinished mappings. Eleven cached upstream commits remain pending.

## AppHost viewport note toggle (partial interaction corpus)

Bytes 59,772–60,795 map to the executed Rust viewport-note toggle test. At
240×32 in split layout, pressing `a` reveals both alpha.ts and beta.ts annotation
summaries and rationales. The fixture uses the source file contents and
annotations (alpha on line two, beta on line one) and additionally verifies
annotations are hidden before the toggle.
Both pinned source tests pass; see [capture and scope](oracles/app-host-viewport-notes.json).
Focused Rust test, formatting and TUI all-target Clippy pass. This reproduces the
source text assertions, not complete cell-frame parity.

At this earlier viewport-note checkpoint, strict audit validated partitions/evidence, then failed with 1,257 files,
1,333 records, 375 translated-test records and 268 unmapped intervals. Eleven
cached upstream commits remain pending; the containing test file is unfinished.

## AppHost custom-theme and agent-skill menus (partial interaction corpus)

Bytes 57,294–58,077 and 58,077–59,772 of the AppHost interaction test file
map to executed Rust tests. The custom-theme case follows F10 → Right → t,
checks the selected custom-theme marker and active label in a 220×20 buffer,
and preserves the configured palette. The agent case follows F10 → three Right
keys → Down → Enter at 120×24, checking intermediate menu labels, guidance,
command and copy text, plus native clipboard-request and dismissal behavior.
Both unchanged pinned source cases pass; see
[source outputs and exact intervals](oracles/app-host-theme-agent-menus.json).
These text/input assertions do not establish complete cell-frame equivalence.
Focused Rust tests, formatting and TUI all-target Clippy pass.

At this earlier menu checkpoint, strict audit validated byte partitions/evidence, then failed with 1,257
files, 1,332 records, 374 translated-test records and 268 unmapped intervals.
Splitting the existing unfinished region adds two mapped records while leaving
its remaining interval unfinished. Eleven cached upstream commits remain pending.

## AppHost experimental-feature reload (partial interaction corpus)

Bytes 53,736–55,659 of `src/ui/AppHost.interactions.test.tsx` map to the
executed Rust queued file-comparison reload regression. Real before/after files,
queued reload and markup-comment requests, and production broker-client registration
replacement verify that reload cannot enable experimental features. The matrix
also covers explicitly disabled launches and reset reloads. Both pinned source
tests pass. See [capture and limits](oracles/app-host-reload-experimental.json).
The broker client remains unstarted, matching the source mock-host scope rather
than claiming live network or terminal-frame parity. Focused Rust test, formatting,
and TUI all-target Clippy pass.

At this earlier experimental-reload checkpoint, strict audit validated partitions and evidence, then failed with 1,257
baseline files, 1,330 records, 372 translated-test records and 268 unmapped
intervals. Eleven cached upstream commits remain pending. No entire AppHost test
file completion or release acceptance is claimed.

## AppHost live-comment reload (partial interaction corpus)

Bytes 51,464–53,736 of `src/ui/AppHost.interactions.test.tsx` map to the executed
Rust queued file-reload test. It loads real files, queues a visible comment,
changes the after-file on disk, commits a queued reload, and checks both new code
and the retained note in a 220×20 terminal frame. Both pinned source runs pass;
see [capture and scope](oracles/app-host-reload-comments.json). TUI all-target
Clippy and formatting pass. No whole-file completion or cell-golden equivalence
is claimed.

At this earlier live-comment checkpoint, strict audit validated the byte partition and evidence, then failed with
1,257 baseline files, 1,330 records, 371 translated-test records and 269 unmapped
intervals. Again, splitting a partially covered file creates additional unfinished
intervals without changing the file count. Eleven cached upstream commits remain
pending; release acceptance is not established.

## AppHost reload root refusal (partial interaction corpus)

Bytes 55,659–57,294 of `src/ui/AppHost.interactions.test.tsx` now map to the
executed Rust owner-thread reload refusal regression. Both unchanged pinned
source tests pass; native coverage additionally checks each outside endpoint and
source directory independently, zero loader calls, and unchanged publication,
selection and mounted/coordinator state. See [capture and limits](oracles/app-host-reload-root.json).
TUI all-target Clippy and formatting pass. This maps only the 1,635-byte test
interval; both surrounding regions and the containing file remain incomplete.

At this earlier root-refusal checkpoint, strict audit validated baseline coverage/evidence, then failed with
1,257 files, 1,328 records, 370 translated-test records and 268 unmapped records.
The unmapped record count increased by one because splitting the old whole-file
interval leaves two unfinished regions. It is not a loss of source files or a
claim of full-file completion. The cached upstream queue still contains 11 commits.

## Historical release report archive

All 22 pinned `benchmarks/release/bench-*.json` reports now have lossless
Rust-generated archive mappings. `cargo xtask benchmark historical-release --check`
and the executable regression verify exact source bytes. See
[archive documentation](../../docs/historical-release-benchmarks.md).
The release benchmark README is separately migrated to
[release snapshot guidance](../../docs/release-benchmark-snapshots.md), retaining
the historical policy while distinguishing unfinished native release gates.
At that archive checkpoint, strict audit retained 1,257 baseline files and 1,326 records, with 267 records
still unmapped and 11 cached upstream commits pending. Historical Hunk numbers
do not establish Workdeck performance acceptance.

The first cached upstream delta has a concrete
[lifecycle-clock gap assessment](upstream-lifecycle-clock-gap.md). Existing native
startup retry injection is partial overlap, not proof that the cross-layer clock
change is already ported; all 11 cached deltas remain pending.

## Native cancellation metadata (partial hook parity)

Native `$/cancelRequest` notifications now distinguish settled requests, explicit
cancellation, and host deadlines through optional `cause` metadata. The SDK accepts
legacy id-only notifications and retains structured JSON `reason` values in the
typed error returned by a cancelled document read. A compiled line-highlighter
fixture exposes the last received notification for executable transport checks.

This does not complete parent abort-reason propagation: the current parent signal
is still an atomic boolean, and no arbitrary parent reason is invented. The full
`useLineHighlights.ts` ledger record remains unmapped. Validation passed 73 SDK
unit tests, 204 host unit tests, and all 24 compiled highlighter integration tests,
including the child-process test distinguishing all three causes. All-target
Clippy with warnings denied passed for the SDK, host and examples; formatting
and whitespace checks passed. This does not supersede the recorded
full-workspace verification.

### Post-response cancellation check

Pinned `runLineHighlightRequest` checks the parent signal again after its awaited
successful result. Native routed requests now recheck the parent after decoding,
before accepting success and selecting cleanup metadata. A deterministic unit
test cancels inside the decoding closure and verifies that success becomes
`HostError::Cancelled`; a companion test preserves uncancelled success and the
original decode failure. All 206 host unit tests, host all-target Clippy with
warnings denied, formatting and whitespace checks pass. This closes that settlement
race, not the outstanding arbitrary parent-reason propagation gap.

### Reason-carrying native parent cancellation

The existing host `ExtensionRequestCancellation` handle now retains the first
JSON-compatible reason under synchronized publication. Clones share that reason;
later aborts and reasonless cleanup cannot overwrite it. The new
`highlight_file_with_cancellation` lazy-document entry point forwards that reason
in native cleanup notifications. Legacy atomic-boolean entry points remain valid.
All 207 host unit tests pass, including first-reason retention. All 25 compiled
highlighter integration tests pass, including nested JSON reason preservation
through actual child-process cleanup. At `e7e9bfa1`, the TUI coordinator still used its
boolean path: integrating its timeout and supersession reasons remains required
before claiming full hook parity.
Host and examples all-target Clippy with warnings denied, formatting, and
whitespace checks also pass for this increment.

### TUI reason-carrying cancellation integration

The preparation coordinator, source-bound runtime, and native runtime adapter now
share host cancellation handles instead of bare atomic flags. Cancelling with a
reason returns the previous cancellation state under the same lock, preserving
the coordinator's completion-versus-supersession decision. Coordinator deadlines
retain `{"name":"Error","message":"highlight timed out"}` through late worker
cleanup. Native host deadlines emit the same reason. Reasonless supersession and
completion retain default-abort semantics; they cannot overwrite an existing
explicit reason. The direct native TUI adapter now uses lazy snapshot reads.

Tests check shared reason identity through source binding, timeout-reason retention
after late completion, and structured reason delivery through the TUI runtime
adapter to a compiled child. Validation passed 207 host unit tests, the existing
1,078-test full TUI unit suite, and all 25 compiled highlighter integration tests.
Host, TUI and examples all-target Clippy with warnings denied, formatting and
whitespace checks pass. A subsequent focused run passed all 44 coordinator tests,
including the additional source-binding identity test. This does not establish whole-hook parity or change any ledger
interval's disposition.

### Four-file ownership during provider contention

Pinned `useLineHighlights.ts` keeps each of four workers on its file until every
provider completes. The native coordinator previously counted only pending RPCs:
four files waiting on a busy second provider could allow the first provider to
start a fifth file. The deterministic regression
`busy_later_provider_retains_four_file_preparation_slots` failed before the fix
with one unexpected pending request instead of zero. Scheduling now counts the
first four unfinished files as occupied slots, including provider contention.
The test also releases contention and verifies all five files eventually finish.
All 45 focused coordinator tests and all 25 compiled highlighter integration
tests pass, along with TUI all-target Clippy with warnings denied, formatting,
and whitespace checks.

Strict audit refreshed after `bf656bd4`: exactly 1,257 baseline files and 1,326
records, with 290 unmapped records and 11 cached upstream delta commits. Audit
exited unsuccessfully on incomplete coverage. No mapping is changed by this fix.

### One deadline across provider waiting and retries

Provider contention previously had no preparation deadline, and a retry created
a fresh full timeout. The coordinator now retains one attempt deadline from the
file's first eligible provider turn through queued waiting, retries, and worker
execution. The waiting map retains only task keys and instants, not frozen file
snapshots. Generation replacement clears those lifetimes; a settled timeout is
cached and cannot invoke its provider later without invalidation.

Deterministic tests expire a queued deadline without starting a worker, verify
reload recovery, and verify that retry execution uses the original deadline.
All 47 coordinator tests and 25 compiled highlighter integration tests pass.
TUI all-target Clippy with warnings denied, formatting and whitespace checks
also pass. No whole-file ledger mapping is claimed.

### Queued-deadline lifecycle regression coverage

Follow-up executable tests cover clearing queued lifetimes on registration removal,
file removal, document reload, and terminal retirement. A separate epoch-replacement
test expires the old queued deadline, replaces its epoch, and verifies a fresh
lifetime without a stale warning, cached failure, or extension invocation.
All 49 coordinator tests pass. These tests extend lifecycle evidence only; they
do not close the whole-hook ledger record.

## Full verifier checkpoint at `1f8d603d`

With the checkout held unchanged, `CARGO_INCREMENTAL=0 cargo xtask verify`
completed successfully. It passed theme assets/notices, skills, architecture,
release fragments, formatting, locked full-workspace all-target tests, locked
workspace all-target Clippy with warnings denied, the optimized sole `workdeck`
executable build, and the large-repository smoke check. Test evidence includes
73 extension SDK tests, 207 host tests, 572 session tests, 24 broker-adapter tests,
1,084 TUI unit tests, 241 VCS tests, 25 compiled highlighter integration tests,
and 198 tooling tests with one existing opt-in test ignored. Native filesystem
watcher tests reported slow execution but completed successfully.

`CARGO_INCREMENTAL=0 cargo deny check` also exited successfully for advisories,
bans, licenses and sources. This used the existing three advisory exceptions in
`deny.toml` (`RUSTSEC-2025-0141`, `RUSTSEC-2024-0320`, `RUSTSEC-2024-0436`) and
reported duplicate-version warnings; neither the exceptions nor dependency
policy were relaxed for the run.

This checkpoint supersedes older repository-verifier results for the changes
through this commit only. It does not establish complete source parity, clear
the 290 unmapped ledger records or 11 cached upstream commits, satisfy the
same-host benchmark gate, or prove native multi-platform CI, signing, archive,
installer, website, or other release gates. No ledger disposition changed.

## Epoch-state generation identity

Pinned `useLineHighlights.ts` lists the epoch-state object as an effect dependency.
A native regression reproduced a hidden-file epoch change leaving a visible
file's old queued deadline active: it timed out rather than restarting unfinished
preparation. The coordinator now retains the shared epoch-state identity for the
generation, in addition to effective per-file cache keys. Clones of that state
do not restart work, and completed visible derivations remain reusable when only
a hidden file's counter changes. This adds one shared handle, not a copied epoch
map. Document replacement releases that handle.

The regression failed before the fix with zero queued attempts instead of one.
All 51 coordinator tests and 25 compiled highlighter integration tests pass,
including completed-cache and shared-clone identity checks. TUI all-target Clippy
with warnings denied, formatting and whitespace checks pass. The full verifier
checkpoint above predates this change. No whole-hook ledger disposition is changed.

## Filter-driven visible-stream generation

Pinned `useTerminalReview.ts` rebuilds `visibleFiles` on filter-text changes, and
the highlighter hook restarts unfinished preparation when that collection changes.
The native review painter now signals the filter text to its preparation owner.
Repeated identical filters retain the current attempt; changed filters retire
unfinished work even when matching file IDs remain equal.

Pinned `mergeFileAnnotationsByFileId` also recreates file objects carrying saved
notes during this rebuild. The painter explicitly discards those files' derived
results while preserving unannotated completed marks. Tests cover equivalent
matches, repeated filters, queued-lifetime retirement, unchanged map identity,
and selective recreation of annotated-file results. All 53 coordinator tests,
25 compiled highlighter integration tests, TUI all-target Clippy with warnings
denied, formatting and whitespace checks pass. The initial annotated-file build
caught an extra reference in a map lookup, which was corrected before these
successful runs. No whole-hook mapping is claimed.

## Saved-note stream generation and streamed identity hashing

The pinned visible-stream memo also depends on the saved-note map. Native
preparation now tracks a digest of that whole projected map: changing a note on
a hidden file restarts unfinished visible preparation, while unchanged plain-file
results remain reusable. On a stream rebuild, current saved-note file projections
discard their completed derivations because the pinned merge creates fresh file
objects. Unchanged note-map content does not restart a request each frame.

`workdeck_core::review_serialized_digest` streams serde JSON into SHA-256 rather
than allocating the serialized payload. Both saved-note-map identity and existing
agent-context identity use this path. A core test verifies byte-exact agreement
with the prior buffered hash for null, Unicode, and a large value, and verifies
serialization errors are returned. This is not a measured performance-gate claim.
All 67 core unit tests, 54 coordinator tests, and 25 compiled highlighter
integration tests pass. Core/TUI all-target Clippy with warnings denied,
formatting and whitespace checks pass. No ledger mapping is changed.

## Additional annotation metadata at the native boundary

Pinned read-only file projection retains saved-note render metadata inside agent
annotations. The native core annotation previously had no place for those fields.
An additive flattened metadata map now preserves additional JSON fields without
adding a wrapper or changing the JSON emitted by annotations with no extra fields.
Existing annotation constructors explicitly initialize an empty map.

`TerminalReviewNote::annotation` now carries stored-note thread metadata and
file/hunk/side/line coordinates. Tests verify legacy-shaped annotations remain
wrapper-free, nested unknown metadata round-trips, mapped user notes retain their
render fields, and native `ExtensionDiffFile` projection retains thread metadata.
The live TUI's separate manual saved-comment projection has not yet been wired
to this richer mapping, so this does not close the end-to-end note projection
gap. Core, review and extension-host unit suites pass, as do workspace-wide
all-target compilation and Clippy with warnings denied, formatting and whitespace
checks. This is not a full workspace test run. No ledger disposition is changed.

### Live saved-note projection

The subsequent TUI integration now uses the shared stored-note mapping and thread
selectors for extension input. Stored anchors, resolution, source and edit authority
are copied without reconstructing them from legacy hunk coordinates. File grouping
accepts borrowed files rather than cloning complete diff payloads. Visibility
collapses thread depth and guides across hidden ancestors without removing the
hidden notes from extension metadata, matching the pinned hook. Drafts, orphaned
notes and notes for absent files remain excluded. Root notes deliberately have no
sibling connector, as in the pinned selector. The legacy canvas fallback is unchanged.

Tests cover user defaults, coordinates, thread metadata, hidden ancestors and stored
anchor authority. The compiled native highlighter fixture returns the annotations
it actually received: the live add/edit/remove test verifies stored-note fields
alongside terminal-cell highlight changes for both eager and deferred sources.
All 175 review unit tests, 1,091 TUI unit tests and 25 compiled highlighter integration
tests pass. This closes the previously noted live saved-note projection gap, not the
full hook parity gate; no source ledger interval is marked complete. Review, TUI and
examples all-target Clippy with warnings denied, formatting and whitespace checks
also pass.

### Visibility-driven preparation generations

Pinned `useTerminalReview.ts` rebuilds the visible-thread map on agent-note visibility
changes and consequently recreates its stored-note map and review stream, even when
annotation content is unchanged. The TUI now signals that visibility identity to the
preparation coordinator independently of its annotation digest. A changed visibility
setting retires queued/running attempts and rederives recreated annotated files;
completed plain-file results remain reusable. Repeating the same setting preserves
the existing attempt deadline. A deterministic coordinator regression covers queued
deadline retirement and completed-result identity retention. No ledger mapping is
changed by this additional lifecycle fix. All 55 coordinator tests and 25 compiled
native highlighter tests pass, as do TUI all-target Clippy with warnings denied,
formatting and whitespace checks.

### Stored collection identity before projection

The preparation stream now hashes saved-note inputs rather than their renderable
annotation projection. Pinned `useTerminalReview.ts` depends on the complete live
and user note collections: changing an orphaned note or a note for an absent file
recreates the stream even if its projected annotations stay empty. Draft notes are
excluded because they belong to a separate source dependency. Regression tests
verify that unprojected saved-note edits change identity, equal saved inputs retain
identity, and draft additions/edits leave it unchanged. The existing coordinator
path retires unfinished work while preserving completed plain-file derivations.
All three saved-extension projection/identity tests and 55 coordinator tests pass,
as do TUI all-target Clippy with warnings denied, formatting and whitespace checks.
This is further partial lifecycle parity, not a completed ledger mapping.

### Full verification checkpoint after stream integration

`CARGO_INCREMENTAL=0 cargo xtask verify` passed at immutable revision `61be18bf`.
The run passed theme/notices, skills, architecture and release-history checks,
formatting, locked full workspace all-target tests, workspace all-target Clippy
with warnings denied, the release `workdeck` build and large-repository smoke test.
Observed suites include all 93 terminal-pager tests, 73 extension SDK tests,
208 extension-host tests, 572 session tests, 24 native broker-adapter tests,
1,093 TUI tests, 241 VCS tests and 25 compiled native highlighter tests. Tooling
finished with 198 passes and one existing ignored test. The watcher tests emitted
long-running notices and then passed; the temporary-ref lock diagnostic in tooling
was followed by a successful suite result.

`cargo deny check` also passed under the unchanged policy. Duplicate-version
warnings remain, as do the existing exceptions for RUSTSEC-2025-0141,
RUSTSEC-2024-0320 and RUSTSEC-2024-0436. This is not a zero-exception claim.
Neither check establishes strict source coverage, upstream catch-up, benchmark
parity, native-platform release evidence, or signed artifact compliance. No ledger
disposition changes at this checkpoint.
A fresh strict `cargo xtask port audit` still fails with 290 unmapped records
across the 1,257-file baseline and 11 cached pending upstream commits.

### Per-file annotation hashing

Highlighter task planning and publication now compute annotation identity once per
eligible file per phase, then reuse the digest across registrations. Previously
each provider repeated the same serialization and hashing in both phases. Empty
registration sets perform no annotation hashing. Cache-key contents, provider
ordering and publication semantics are unchanged. All 55 coordinator tests pass
after the final change; TUI all-target Clippy with warnings denied, formatting and
whitespace checks also pass. This is a local reduction in repeated work, not measured
benchmark parity, and it does not change any ledger disposition.

### Empty highlighter registry frame path

The live frame checks the line-highlighter registry before building saved-note
projections, merged file views, or source-bound runtime wrappers. With no providers,
it still reconciles the preparation controller against an empty generation to retire
old work and published extension marks, then preserves agent-provided highlights
through the existing merge function. Registered-provider behavior is unchanged.
All 1,093 TUI unit tests, TUI all-target Clippy with warnings denied, formatting and
whitespace checks pass. This does not complete a ledger interval or performance gate.

### Immutable-document gap geometry reuse

The eligible plain split-stream cache now retains the small gap descriptors for
each file alongside section layouts. Viewport row construction borrows those
descriptors instead of recounting full old/new source lines and rebuilding hunk
gap metadata for every file on every frame. The same retained immutable document
and settings checks invalidate both caches. The uncached painter and complex-layout
paths retain their existing geometry calculation. Initial cache construction pays
an additional descriptor pass; steady-state frames reuse it. Row construction,
highlight prefetch, navigation metadata and mouse targets are not skipped.

The viewport differential test now compares cached-gap output with the uncached
painter across widths, Unicode, hunk headers and viewport ranges, including complete
navigation geometry. Cache tests verify descriptor identity reuse, equality with
fresh geometry, resize invalidation and full-cell equality after forced rebuilding.
All 1,093 TUI unit tests, all-target TUI Clippy with warnings denied, formatting and
whitespace checks pass. Timing improvement remains unmeasured at this increment;
offscreen row construction and the performance gate remain unresolved. No ledger
interval is newly mapped.

### Immutable split-plan reuse

The plain-file geometry cache now also retains each hunk's split-line index pairs.
Viewport rendering borrows those plans instead of rerunning the unchanged alignment
algorithm and allocating pair vectors for every hunk each frame. Uncached and complex
paths still calculate their own plans. The document-owned cache is invalidated with
the existing section geometry, and retains index metadata rather than copied line
text. This adds retained index storage and initial construction work; its timing and
memory effects must be measured, not assumed.

Cached/uncached viewport tests still compare visible styling and complete line cursor,
note target, file and hunk geometry. Cache reuse/invalidation and full-cell tests also
pass. All 1,093 TUI unit tests, TUI all-target Clippy with warnings denied, formatting
and whitespace checks pass. The initial struct extraction had a misplaced Debug
derive, corrected before this successful validation. No ledger mapping or benchmark
gate is completed by this increment.

### Split-row vector reservation

The split hunk renderer reserves row and note-target capacity from the computed
pair count and cursor capacity from the source-line count before appending rows.
Wrapping and note rendering can grow these buffers normally; no content, targets,
or geometry are omitted. This avoids repeated vector growth in ordinary unwrapped
hunks, including offscreen geometry rows. All 1,093 TUI tests, all-target TUI Clippy
with warnings denied, formatting and whitespace checks pass. Timing and memory
effects still require measurement; no ledger or performance gate is completed.

### Frame-wide row capacity hint

The plain viewport path passes its already-computed content height as an initial
capacity hint for frame-wide rows, note targets and line cursors. Those vectors
previously grew from empty on every frame. This is allocation planning only: the
hint never limits output, and vectors can grow for additional cursor entries.
The complete row-building, note-target and navigation behavior is unchanged.
All 1,093 TUI tests, all-target TUI Clippy with warnings denied, formatting and
whitespace checks pass. The viewport differential test exercises the hint while
comparing visible cells and complete geometry to the uncached painter. Performance
and peak-memory effects remain to be measured; no source ledger interval is mapped.

### Compact ordered note-target lookup

Frame note targets now retain their sorted vector representation instead of being
converted into a BTreeMap each frame. Exact-row lookup uses binary search; ordered
iteration is unchanged. Unordered inputs use stable sorting, and duplicate rows keep
the last inserted value, including collisions after composer row shifts. No target
is dropped except duplicates already replaced by the previous map semantics.

A direct differential test compares ordering, lookup, duplicate handling, extreme
row addresses and shifted collisions against BTreeMap. All 1,094 TUI tests pass,
including full-cell/geometry and composer coverage. TUI all-target Clippy with
warnings denied, formatting and whitespace checks pass. Performance effects remain
to be measured at this code checkpoint; no ledger interval is newly mapped.

### Executable pager regression checkpoint

After compact note-target lookup and the rendering cache changes,
`CARGO_INCREMENTAL=0 cargo test -p workdeck-cli --test terminal_pager` passed
all 93 tests at immutable revision `9234fcb5`. This adds real-executable terminal
integration evidence to the preceding library full-cell/geometry checks. The
checkout was unchanged throughout the build and test run. It is not a new full
workspace verifier run, a paired benchmark pass, or native-platform release
validation; source-ledger coverage and upstream catch-up remain unresolved.

### Exhaustive short note-target sequences

The compact lookup differential test now enumerates all 3,280 sequences of length
zero through seven over row addresses 0, 2 and 8, plus four explicit edge sequences.
Each input checks ordered iteration and exact lookup against BTreeMap, then twelve
combinations of saturating removal/insertion shifts, including usize maximum values.
This exercises duplicate order, partial collisions and total collapse without random
seeds. The expanded test passes, as do TUI all-target Clippy with warnings denied,
formatting and whitespace checks. This test-only increment is not another full TUI
suite run and does not change the source ledger or performance evidence.

## Geometry-memory workload (partial tooling port)

`cargo xtask benchmark geometry-memory` now runs the pinned default 180-file,
120-line, width-240 geometry workload. It measures fixture construction, retains
all file geometries, samples memory before and after lazy row-plan materialization,
and then separately times first-copy materialization for the 50,000-line giant
fixture. The giant fixture is constructed only after the ordinary memory samples.
Outputs include all three native RSS/malloc snapshots and row counts; values are
not relabeled as JavaScript heap size, extra memory or object counts, and no GC is
claimed. The current native memory backend supports macOS only.

A reduced-fixture executable unit test verifies lazy plans, materialized row counts
and the giant-copy path. That test and xtask all-target Clippy with warnings denied
pass. Source CLI flags/help, source-oracle comparison and cross-runtime memory
acceptance remain incomplete. The command currently rejects arguments explicitly;
the full `benchmarks/geometry-memory.ts` ledger record remains unmapped.

### Geometry-memory options

The subsequent options port accepts `--file-count`, `--lines-per-file`, `--width`,
`--no-gc`, `--help` and `-h`. It uses source-style numeric coercion, rejects negative
and non-finite numbers, truncates fractional values and clamps counts to one and
width to forty. Repeated options use the last value; help short-circuits subsequent
arguments. Native size conversion happens after parsing so help or a later value
can supersede a large finite input. Final unaddressable native sizes are rejected
explicitly rather than saturating a Rust cast.

The output distinguishes `sourceGcRequested` from `nativeForcedGc: false`; native
allocation is not represented as JavaScript collection. Help works before querying
the platform memory backend. Both geometry-memory tests, xtask all-target Clippy
with warnings denied, formatting and whitespace checks pass. The source help is
adapted to Cargo naming and native GC semantics. Frozen option oracles, cross-runtime
heap equivalence and unsupported-platform memory backends remain open; the ledger
record is still unmapped. The earlier no-arguments limitation is superseded.

## Native request-ID exhaustion

The host no longer saturates and reuses its final request ID. Checked allocation
returns an explicit error before writing when the ID space is exhausted; the
cancellable path's existing send-failure cleanup retires its reserved inbox.
Tests verify normal allocation, the last allocatable ID, and repeated failure
without wrap or reuse. This protects routing identity and adds no ledger coverage.

## Pending-state compatibility during concurrent highlighting

The general pending probe also uses a nonblocking route-lock check: contention
reports busy rather than blocking the UI behind stdout dispatch. A bounded
contention regression holds that lock, verifies the probe returns before release,
and checks idle and active-parent states. This is responsiveness evidence for
the probe, not the complete latency benchmark gate.

General `request_pending()` once again reports active routed highlighters as
busy to existing pane/command UI callers. The highlighter coordinator uses a
separate `line_highlight_request_pending()` check that permits other routed
parents while excluding a legacy request. The compiled held-read test asserts
both observations before cancellation or timeout. Verification passes: all 17
compiled highlighter tests, all 1,072 TUI unit tests, workspace Clippy, formatting,
and architecture checks. This preserves the public busy signal without disabling
concurrency. The fresh strict audit still fails on 313 unmapped records and
reports 11 cached upstream commits; no coverage is added.

## Closed native transport is not retryable contention

The four-parent fixture can now exit only after receiving all four invocations.
Its new integration case requires every waiting host caller to receive Closed,
not Timeout or Busy. All seventeen compiled integration tests, examples Clippy,
formatting, and architecture checks pass. This is transport EOF coverage, not
an assertion about forced crashes or all retirement interleavings.

A compiled fixture exits when it receives a highlight invocation. Its regression
requires both the interrupted call and a subsequent call to return
`HostError::Closed`. The first run failed because parent-inbox registration
mapped every route error to Busy. Closed registries now retain their terminal
error instead of inviting retries. All sixteen compiled integration tests pass,
along with examples Clippy, formatting, and architecture checks. This does not
establish all crash, malformed-frame, or lifecycle semantics.
No ledger coverage is added.

## Bounded native stdout frames

The host stdout reader now bounds each frame before JSON parsing or response
routing, accepting at most `MAX_MESSAGE_BYTES` payload bytes plus a newline.
Oversized, unterminated, and invalid UTF-8 frames return an input error and close
the response routes through the existing terminal path. Unit tests verify exact
limit acceptance, preserved CRLF/LF frames, EOF, and consumption limited to
`MAX_MESSAGE_BYTES + 2` bytes on oversized unterminated input. The stdout bound is
separate from the stderr limits below; it does not bound the legacy response queue
and is not a whole-process memory gate.
Verification passes: all fifteen compiled highlighter integration tests, all
180 host unit tests, host Clippy, formatting, and architecture checks. No ledger
coverage is added.

## Bounded native stderr diagnostics

The host now drains stderr with a bounded line reader rather than growing a buffer
until a newline arrives. Normal LF/CRLF, empty lines, lossy UTF-8 and final EOF fragments
retain their previous behavior. Captured messages are limited to 16 KiB of UTF-8 text;
oversized lines include an explicit `... [truncated]` suffix. The capture loop drains
the remainder of oversized lines instead of closing the pipe or treating stderr as
protocol output.

One shared log hub retains the newest 1,024 entries within a 1 MiB UTF-8 payload
budget that includes extension IDs. `ExtensionLogHub::stats` and
`LoadedExtension::log_stats` expose retained counts/bytes, discarded-entry counts and
truncated-line counts. The budget is a payload bound, not a whole-process allocator
or peak-memory benchmark claim. Reader scratch space and entry metadata are separately
bounded by line and entry limits. Tests cover exact limits, overflow followed by a new
line, long unterminated invalid UTF-8, concurrent writers, identity-byte accounting,
and continued drainage after retention is exhausted.

The compiled line-highlighter diagnostic fixture writes a 4 MiB line followed by
1,100 short lines and a marker before sending its protocol reply. Its integration test
requires a successful reply, exactly 1,024 retained entries, 78 discarded entries,
one truncated line, the final marker, and a successful subsequent native request.
This closes stderr accumulation only. The serialized response-queue bound is described
below; stdin write deadlines remain unfinished. No source-ledger record is marked complete here.
All 192 host unit tests and 19 compiled highlighter integration tests pass after
this increment, along with host/examples all-target Clippy, formatting, and the
architecture check. The earlier full verification at `c10ab4e8` predates these log
changes and is not presented as a full-workspace verification of this increment.

## Bounded serialized native response queue

The serialized native response queue is now limited to eight complete frames,
independently of the existing per-frame size limit. Its dedicated stdout reader
applies backpressure until the consumer drains space; it does not drop valid frames
or block while holding the routed-parent registry lock. Receiver destruction releases
a reader waiting on the full queue. Each serialized request owns a lease, including
asynchronous commands and events until polling completes. Returning, cancelling or
retiring that request releases the lease, clears revoked queued output and wakes
the reader. Recognised old replies and CLI output for another request are discarded
before they can fill a newer lease. Unowned malformed frames terminate the reader;
they are not silently ignored. This "legacy" queue means serialized native
extension operations, not the separate old Workdeck per-TUI session listener.
Unit tests check exact capacity, frame/error order and receiver-drop release. A
compiled CLI fixture emits 128 frames while the host delays its first output write;
the test requires byte-exact output, a successful result and a subsequent command.
A second compiled fixture emits 128 late replies after settling its request. Its test
requires the next routed highlighter and serialized query to complete, guarding against
an idle queue holding up the shared stdout reader. An initial assertion incorrectly
expected a null annotation-width diagnostic after highlighting; the fixture records
width 3 on that path, and the assertion was corrected from the fixture implementation.
At this queue increment, stdin writes still preceded their response deadlines;
the later Unix write work is described below. The queue bound alone is not a
claim that every native transport operation is deadline-safe.

The initial three queue tests and 195 host tests passed before request leases were added.
The first compiled CLI run failed seven initial handshakes
with timeouts, before invoking the output-burst action. The burst test then passed
in isolation, and a rerun of all eight CLI and nineteen highlighter integration tests
passed without changing deadlines or assertions. The initial handshake timeouts are
not claimed fixed or causally explained. No source ledger mapping is added.

Final lease-based validation passes all 198 host unit tests and all 59 focused native
integration tests: eight CLI, twenty highlighter, seventeen sidebar, eleven workspace
and three startup-lifecycle tests. Host/examples all-target Clippy, formatting, diff
and architecture checks pass. The earlier full workspace verification predates this
queue change; this focused validation does not satisfy the remaining full-port gates.

Subsequently, `CARGO_INCREMENTAL=0 cargo xtask verify` passed at `e3c3d795`,
covering the committed log and response-queue changes together. The run passed
theme/notices, skills, architecture and release-content checks; workspace all-target
tests (including 93 terminal-pager, 198 extension-host, 1,074 TUI and 241 VCS tests);
workspace Clippy with warnings denied; the optimized release build; and the
large-repository smoke test. The tooling suite passed 198 tests with its one existing
ignored test. Strict port accounting remains 290 unmapped records and 11 cached
upstream commits; this local verification is not a release or full-parity approval.

### Remaining native stdin deadline work

At `b4eb78b6`, `request` and `request_cancellable` started response deadlines after
`send_request_on` writes and flushes the child pipe. CLI, asynchronous command/event,
notification and document-response paths also used blocking frame writes. Moreover,
`begin_retirement` could write a shutdown notification despite its nonblocking contract.
The replacement must cover the write itself, preserve frame ordering,
and retire a partially written stream rather than append a new frame after timeout.
Cancellation/shutdown must not introduce a second blocking write on the same pipe.

Platform constraints were checked against primary documentation: POSIX
[nonblocking pipe writes](https://pubs.opengroup.org/onlinepubs/9699919799/functions/write.html)
can return partial progress or `EAGAIN`; readiness from
[poll](https://pubs.opengroup.org/onlinepubs/9799919799/functions/poll.html)
does not make a subsequent large blocking write safe. On Windows,
[anonymous pipes do not support overlapped I/O](https://learn.microsoft.com/en-us/windows/win32/ipc/anonymous-pipe-operations),
and [CancelSynchronousIo](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelsynchronousio)
does not itself wait for cancellation completion. Merely moving a blocking write to
an unjoined worker is therefore not accepted as deadline-safe transport. A replacement
must retain the existing owned-pipe/thread-scoped SIGPIPE protections, test a child
that stops reading, and verify shutdown plus subsequent-request rejection after a
partial-frame timeout.

The subsequent Unix implementation configures the parent's owned stdin endpoint
with `O_NONBLOCK`, retaining Darwin's per-pipe SIGPIPE protection and the existing
thread-scoped signal guard elsewhere. Short writes preserve their offset; full pipes
wait in at most 2 ms slices against the same deadline as the response. Cancellable
writes check the flag between attempts. Document-response mutex acquisition uses
that same budget. Shutdown and expired/cancelled cleanup never wait for capacity;
successful request cleanup can use the unexpired original deadline.

Any partial-frame failure or transport I/O error makes the stream permanently
unavailable, revokes runtime authority, closes routed parents and starts child
termination. A cancellation or deadline before the first byte leaves the stream
intact and does not revoke unrelated routed parents. Subsequent writes
cannot append JSON to a partial frame. Host tests include a real nonreading child
with an observed nonzero partial write, plus short-write, backpressure, deadline
and cancellation tests. The compiled highlighter fixture exercises a stopped reader
through public host APIs. At that increment Windows still needed a proper cancellable
replacement; no cross-platform deadline parity or new source ledger mapping was claimed.

Validation passes 203 host unit tests and 62 compiled integration tests (17 sidebar,
11 workspace, eight CLI, 23 highlighter and three startup-lifecycle tests), plus
host/examples all-target Clippy with warnings denied, formatting, diff and architecture
checks. During development, three new integration tests failed: two assumed their
3 MiB JSON frame had reached the pipe before a 100 ms budget expired, and one used
the highlighter-exclusion indicator instead of routed-request ownership. The tests
now use a 512 KiB saturation frame with a 500 ms request deadline / 200 ms cancellation
trigger, and wait for a routed parent without a serialized writer holding its lock.
The strict rejection-after-partial-write assertions remain; the lower-level real pipe
test independently observes a nonzero partial write before timeout. No production
timeout constant was increased to make these tests pass. Pre-write cancellation and
a zero deadline are separately required to preserve an intact stream and its peer.
The full verification at `e3c3d795` predates this Unix write change; focused validation
does not substitute for final workspace, benchmark or native cross-platform gates.

### Windows nonblocking stdin implementation and outstanding native validation

Windows now creates stdin with `std::io::pipe` rather than `Stdio::piped`: Rust
1.95's [standard anonymous pipe](https://github.com/rust-lang/rust/blob/1.95.0/library/std/src/sys/pipe/windows.rs)
uses synchronous `CreatePipe` handles, whereas its
[child pipe](https://github.com/rust-lang/rust/blob/1.95.0/library/std/src/sys/process/windows/child_pipe.rs)
uses overlapped I/O internally. The host passes the blocking reader directly through
`Stdio::Handle` and drops its `Command` after spawning, releasing the parent's extra
reader before the handshake. Only the retained writer is configured with
`SetNamedPipeHandleState(PIPE_NOWAIT | PIPE_READMODE_BYTE)`. Failure to configure
the mode fails startup; there is no fallback to an indefinitely blocking writer.

Microsoft documents that [the mode setter accepts anonymous pipe handles](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-setnamedpipehandlestate)
and that [nonblocking byte writes return available progress immediately](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-type-read-and-wait-modes).
Its warning against using this legacy mode to achieve overlapped/asynchronous I/O
is retained here: Workdeck uses synchronous, bounded write attempts, not background
overlapped operations. Successful zero-byte writes on a full nonempty pipe become
`WouldBlock`, not `WriteZero`; empty writes, short counts and actual errors are preserved.
The same deadline/cancellation loop tracks accepted bytes and retires partial frames.
Flushing is a no-op for this unbuffered endpoint, not `FlushFileBuffers` (which can
wait for the child to drain it). No relay thread or pending borrowed buffer is introduced.

Three Windows-only tests cover independent endpoint modes and round-trip bytes,
partial progress then zero-progress backpressure and recovery, and reader closure.
The compiled stopped-reader deadline and cancellation tests are no longer Unix-only;
the existing native Windows CI job runs them through workspace all-target tests.

The full host cross-check for `x86_64-pc-windows-gnu` stopped in `onig_sys` because
this Mac has no `x86_64-w64-mingw32-gcc`. An isolated disposable Cargo harness includes
the actual `child_pipe.rs` by path, pins `windows-sys` 0.61.2 with the same features,
and checks that module, its tests and `Command::stdin(reader)` composition for the
Windows target. That type-check passes; it does not execute the tests or validate
the complete Windows host. Native Windows CI/execution remains required. No upstream
commit or baseline interval is marked complete on the strength of this check.

Local validation passes 204 host unit tests and 34 compiled integration tests
(eight CLI, 23 highlighter and three startup-lifecycle tests), host/examples
all-target Clippy with warnings denied, formatting, diff and architecture checks.
The isolated Windows module/tests/composition harness also passes cross-target
Clippy with warnings denied, including the connection's `Send` and `Debug`
requirements. This is focused validation, not a new full-workspace verification
or a native Windows execution result.

## Borrowed highlighter planning and warning retention

Inspection of the pinned `useLineHighlights.ts` and its Rust coordinator found
full `DiffFile` clones in every desired-task planning pass, including cache hits
and busy/unstarted requests. Planning now borrows the live files. A task takes one
owned immutable snapshot only when it starts; the worker, deadline and completion
records share that snapshot through `Arc`. Merged-cap reporting also borrows its
file rather than cloning the document merely to deduplicate a warning.

An executable ownership regression checks that every planned registration points
at the original file, that worker/deadline snapshots share one allocation, and that
later edits to the original cannot mutate the worker snapshot. A separate test
checks the pinned 256-key FIFO warning bound, oldest-key eviction, and duplicate
reports not refreshing a key's age. This establishes those contracts, not a measured
latency or peak-memory improvement. The full hook interval remains unmapped pending
its complete semantic/evidence review.

Validation passes all 41 highlighter coordinator tests, all 1,076 TUI unit tests,
23 compiled native highlighter integration tests, TUI all-target Clippy with
warnings denied, formatting, diff and architecture checks. No same-host benchmark
result or new full-workspace verification is inferred from these checks.

## Highlighter registration removal contracts

Two additional executable coordinator regressions cover the pinned hook's
no-highlighters branch. Removing all registrations clears cached derivations and
published marks; repeated empty reconciliation retains the empty map identity.
Re-adding the same registration invokes it again and produces fresh mark storage.
Warning history, unlike cached results, survives removal: the same failing
registration runs again but does not repeat its already-reported warning.
All 43 highlighter coordinator tests, TUI all-target Clippy with warnings denied,
formatting and diff checks pass. These checks strengthen the pending
whole-hook audit without marking its full source interval complete.

## Native concurrency parity gap confirmed

The current transport releases its connection lock while cancellable requests
wait in parent-specific inboxes, retaining serialized writes. A compiled fixture
holds four invocations in one child before replying in reverse order; each host
caller checks its own file path. The initial unlocked implementation regressed
saved-note highlighting because an ordinary query could enter a synchronous
document wait. Legacy requests, CLI commands, commands, and events now remain
Busy until routed parents finish cleanup. This preserves their serialized
behavior while allowing independent highlighter waits.

Final verification passes after all legacy guards: twelve compiled highlighter
tests, all 178 host unit tests, workspace Clippy, formatting, and architecture
checks. The integration suite includes both the four-parent case and saved notes.
An additional compiled case waits for all four parents, replies to one as a
readiness signal, and keeps the remaining three unresolved until a specified
parent is cancelled. The cancelled caller receives `HostError::Cancelled`; the
other two receive their own results after the child consumes that cancellation.
All thirteen integration tests pass, including the original reversed-response
case. This proves isolation for that cancellation sequence, not concurrent
document callbacks or arbitrary event interleavings.
The compiled fixture now also sends four document callbacks after all four
parents arrive. Unique child IDs map replies back to their parent, and each host
reader returns distinct text keyed to its captured file. All fourteen compiled
integration tests pass, including checks that each parent receives its own path
and source text. This covers concurrent callbacks in that fixture, not mixed
callback cancellation/errors or a reusable multiplexed SDK.
A multiplexed SDK remains unfinished. No complete concurrency parity or
ledger coverage is claimed.

The concurrent-callback fixture also has a mixed-result regression: one captured
source provider returns an ordinary error while three return distinct text.
Only that parent's document result may become null; every peer must retain its
own source and file path. All fifteen compiled highlighter tests pass, along
with examples Clippy, formatting, and architecture checks. This does not cover
combined callback cancellation/failure or complete native lifecycle parity.

The host now has a parent-route primitive with four bounded inboxes and
nonblocking dispatch. Tests cover out-of-order delivery, independent retirement,
duplicate/limit rejection, overflow isolation, and disconnected consumers.
Frame classification routes document callbacks by `parentRequestId` and ordinary
responses by response ID. A collision regression uses a child callback ID equal
to another active parent and verifies that only the owning parent's inbox gets
the callback. Notifications, malformed JSON/version/parent IDs, and unknown
parents stay with the caller for explicit legacy or error handling. Routing does
not itself validate callback authority or payloads.
Unknown/retired parent frames return to the caller for legacy dispatch or stale
rejection. Frame-byte validation and precise terminal-error propagation remain
dispatcher work.
The stdout reader now consults these routes before forwarding unmatched frames
to the existing response channel. EOF, reader failure, and connection teardown
close the route registry. Legacy request methods retain their original inbox.
The cancellable request path now registers its parent inbox before sending,
receives responses/callbacks through that inbox, and retires it on send failure
or request completion after cleanup. Transport failures disconnect routed
waiters; more precise transport-error propagation remains follow-up work.
This addresses response-wait serialization without adding ledger coverage.
The route primitive also has idempotent terminal closure: it disconnects all
parent inboxes after buffered frames drain and permanently rejects registration.
Tests cover both empty and buffered waiters. Stdout EOF/error handling now invokes
this lifecycle; active parent transport behavior still needs compiled coverage.

The original gap was identified by inspecting pinned `useLineHighlights.ts`: four file workers
sharing the same registered highlighter. The Rust coordinator's corresponding
test uses `FakeLineHighlightRuntime`; it does not establish native process
concurrency. Previously `LoadedExtension::request_cancellable` held the connection mutex
through the response loop, `try_connection` returned Busy under contention, and
`request_pending` treated that lock as an in-flight request. Consequently one
held native calculation prevented other files from invoking that same extension.
The baseline highlighter hook remains unmapped; prior four-worker unit evidence
must not be used as native concurrency parity evidence.

The remaining transport work must preserve independent cancellation/deadlines
and request-local document authority across concurrent parent calculations.
Child callback response IDs must be unambiguous across simultaneous parents;
the synchronous SDK/example does not currently provide a multiplexed serving
loop. The four-parent reversed-response fixture now passes; a further compiled
case must verify cancellation of one does not retire the others. Additional
processes per file would duplicate extension lifecycle/state
and are not a substitute for this behavior. No ledger coverage is added.

## Native document timeout during a held source read

The compiled held-read fixture now exercises both explicit cancellation and the
host's real highlighter deadline. The timeout case requires `HostError::Timeout`
before the source is released, then completes an ordinary request while source
I/O is still held. After release, the retained shared reader returns its text
and another ordinary request succeeds. This verifies parent retirement without
cancelling shared I/O or contaminating subsequent protocol exchanges. It does
not verify deadlines during blocked pipe writes or complete lifecycle parity.
No ledger mapping is added.
Verification passes: all eleven compiled highlighter tests, examples Clippy
with all targets, formatting, and architecture checks.

## Synchronous native document SDK

Error-response validation rejects simultaneous result/error fields, null error
objects, and malformed error codes/messages. A well-formed host rejection remains
distinct from invalid protocol data. The five SDK tests cover these cases in
addition to frame bounds, source results, and parent cancellation; this adds no
ledger coverage.

Boundary regressions accept a JSON payload exactly at `MAX_MESSAGE_BYTES`,
reject one extra byte, and prove an oversized unterminated stream consumes only
`MAX_MESSAGE_BYTES + 2` bytes before rejection. Additional cases distinguish EOF
and ignore cleanup for another parent. These checks verify frame consumption,
not peak process memory or the repository-wide benchmark gate.

`workdeck-extension-api::read_extension_document` extracts the example's
callback client into the native SDK. It accepts either side, uses the typed
parent-bound wire request, bounds incoming frame allocation, validates response
version/ID/result shape, and returns interruption for matching parent cleanup.
The compiled example now delegates to it. Unit tests cover old-side encoding,
Unicode text, null, malformed frames, mismatched IDs, and cancellation.

This helper requires exclusive use of a single-request stream. It cannot impose
a deadline on arbitrary blocking I/O and is not an asynchronous/multiplexed SDK.
No complete SDK parity or ledger coverage is claimed.
Verification passes: two SDK unit tests, all ten compiled highlighter integration
tests using the extracted helper, workspace Clippy, formatting, and architecture
checks.

## Unreadable native document regression

A compiled highlighter fixture now requires an unreadable lazy document and
returns an empty mark array after its duplicate callbacks. The integration test
injects an ordinary provider error while the file still has embedded snapshots:
the child must observe null, not a fallback snapshot or a failed parent request.
Two separate parent readers each fetch once despite duplicate callbacks, proving
that failure deduplication is request-scoped and cleanup permits a later parent.
This does not establish exhaustive source-error or callback-protocol parity and
adds no ledger coverage.
Verification passes: all ten compiled highlighter integration tests, workspace
Clippy, formatting, and architecture checks. A fresh local strict audit still
fails with 313 unmapped records; upstream catch-up remains incomplete.

## No-read native highlighter regression

The compiled example has a `skipDocuments` fixture mode that returns an empty
mark array without issuing a document callback. Its integration test performs
two lazy invocations with required lifecycle cleanup, checks settled host state,
and verifies the captured provider was never called. This verifies the native
host's no-read behavior; it does not by itself prove every TUI preparation path
or complete callback/SDK parity. No ledger mapping is added.
Verification passes: all nine compiled highlighter integration tests, workspace
Clippy, formatting, and architecture checks.

## Live TUI lazy highlighter source binding

The source-bound TUI runtime now builds a request-local document reader from
the captured VCS capability, falling back to immutable file snapshots only when
no capability is bound. It passes this reader to the native host instead of
materializing both source sides before invoking the extension. The shared
reader maps provider errors to unreadable results and retains per-request
deduplication; provider capability caches remain bound to their captured source.

The compiled saved-note test now rejects old-side source reads and expects only
one cached new-side fetch across note creation, editing, removal, and filtering.
Its existing zero-read construction and terminal-cell assertions remain intact.
The test runtime explicitly requests both sides for older source-ownership unit
tests; production native requests do not take that test-only path. No ledger
coverage or full callback/SDK parity is claimed by this integration.
Verification passes: all 1,072 TUI unit tests, 39 focused highlighter tests,
all eight compiled highlighter integration tests, workspace Clippy, formatting,
and architecture checks. General SDK support and exhaustive callback failure,
deadline, and no-read behavior remain separate verification work.

## Native highlighter document callback transport

The transport drains ready callbacks one at a time, checking cancellation and
the parent deadline before copying each response. This avoids collecting up to
32 copies of source text before those checks. A broker regression verifies that
each drain removes exactly one request and keeps all completed IDs spent. This
is a bounded-allocation change, not evidence for the overall benchmark gate.

The callback capability is now a typed `LineHighlightRequest.document_reader`
field shared by the host and compiled example. Omitted capability fields decode
as false and the eager wire shape remains unchanged. Wire tests round-trip both
modes and reject a string-valued capability. This does not switch the live TUI
to lazy reads or supply a general SDK callback helper.

The cancellable JSON-RPC receive loop now serves parent-bound document requests
through the shared reader. The explicit lazy highlighter entry point advertises
`documentReader: true` without embedding source text. Responses respect the
message-size limit; parent cancellation, timeout, success, and failure retire
the broker and send best-effort child cleanup.

The compiled example requests only the new side, twice. Its integration test
verifies one source fetch, no old-side fetch, and a successful ordinary request
afterward. A held-read cancellation test verifies prompt caller cancellation,
continued child responsiveness, and completion of the retained shared read
without publishing a stale callback into a later request.

All eight compiled highlighter tests and workspace Clippy pass. This connects
the host transport and example, not the live TUI source-bound runtime or a
general executable SDK callback helper. Those still require integration;
no ledger mapping or complete lazy-read parity is claimed.

## Parent-bound document callback broker

The host now has a request-scoped broker around its existing shared document
reader. Wrong-parent and duplicate child IDs are rejected before a source read
can start. Pending callbacks are capped at 32 and total child IDs at 256 per
parent; completed IDs cannot be replayed. Nonblocking polling returns ready
results, including unreadable sides, and retirement revokes publication and
new requests without cancelling an underlying shared read.

The native wire model for `workdeck/document/read` carries only `parentRequestId`
and `side`. Unknown fields (including paths), invalid sides, negative IDs, and
string IDs are rejected. Tests exercise the authority checks, bounded queues,
same-side deduplication, retirement, late shared completion, and wire shape.

This is the broker and wire contract, not an active highlighter transport:
the JSON-RPC receive loop and executable SDK/example still need connection to
this broker. No lazy-read parity or new ledger coverage is claimed.
Verification passes: two broker tests, the strict wire-model test, workspace
Clippy, formatting, and architecture checks.

## Lazy document protocol bridge preparation

Pinned `extensionDocumentReader.ts` starts a source read only when a side is
requested, deduplicates it for the request lifetime, maps unreadable sources to
null, and aborts individual waits without cancelling the shared read. The host
already has that shared-reader primitive, but native highlighter requests still
embed eagerly loaded old/new documents. That transport is not lazy-read parity.

`ExtensionDocumentRead::try_result` now lets a protocol loop inspect a shared
read without waiting on I/O or a contended result mutex. Its result distinguishes
not-yet-available data from a settled unreadable side. Tests verify contention,
shared completion after caller cancellation, exact text, and unreadable results.

The next bridge must use the existing reader rather than duplicate its cache,
bind child document requests to the live parent request ID and captured file
authority, bound pending child requests, and keep processing cancellation and
deadlines while reads run. No native highlighter callback protocol is claimed
implemented by this primitive, and no ledger coverage is added.
Verification passes: all four reader tests, workspace Clippy plus a host-only
recheck after bounding the contention test, formatting, and architecture checks.

## Note-enriched views with deferred source authority

The source-bound highlighter regression now merges a live annotation before
invocation. It checks zero reads during view construction, unchanged source
identity, lookup through the original capability registry, both source sides
and the note arriving together at the consumer, cached side reads, and an
unchanged serialized review document.

The compiled note-transition test runs with both embedded snapshots and a
deferred VCS provider. The deferred provider asserts its captured request path;
the test checks zero reads through app construction and exactly two side reads
across creation, editing, removal, and draft/orphan filtering. Terminal marks
and the child's observation query still verify every transition. This extends
the preceding integration evidence without adding a ledger mapping or claiming
complete provider, hook, or performance parity.

The first deferred integration run failed its post-construction zero-read
assertion: cursor seeding requested geometry through a path that also started
highlighter workers. Geometry-only row and reveal queries now consume already
published marks; paint paths remain responsible for preparation. This keeps
cursor initialization from starting highlighter source I/O.
Final verification passes: all six compiled highlighter tests (the note case
runs embedded and deferred sources), all 1,072 TUI unit tests, workspace Clippy,
formatting, and architecture checks. Lazy native `readDocument` parity remains
unproven; these tests verify captured-reader ownership and caching.

## Saved-comment inputs for native highlighters

The live highlighter path now receives saved comments from the caller's held
review state. It groups non-draft, non-orphaned notes by file identity, projects
the same annotation fields used by the note painter, and merges them into
consumer-owned file views before filtering and preparation. Preferred source
lines survive projection even when a note has no hunk range. Existing summaries
and baseline annotations remain intact; the merged agent path names its file.

The shared merge helper now offers borrowed results: untouched files retain
their original references, while annotated files receive owned copies. The
immutable changeset and its source identity are not rewritten. Agent-context
cache identity causes creation, edits, and removal to rederive native marks.

The compiled example has an opt-in annotation-driven mode and an observation
query. Its integration test checks terminal mark widths and the child's observed
inputs across note creation, edit, removal, and a mixture of active, draft, and
orphaned notes, then verifies the original review document is unchanged.
This implements the invocation gap identified below, not complete hook parity
or deferred-provider/source-authority coverage. No ledger mapping is added.
Verification passes on the final implementation: all six compiled highlighter
tests, all 1,072 TUI unit tests, workspace Clippy, formatting, and architecture
checks.

## Live-comment highlighter integration audit

The five compiled highlighter integration tests pass at `5f2ddce0`, including
native lifecycle cleanup and the Ratatui cell-buffer case. They do not prove
live-comment input parity. Pinned `useTerminalReview.ts` derives `visibleFiles`
through `buildReviewStreamState`, which first calls
`mergeFileAnnotationsByFileId`. `App.tsx` passes that merged stream to
`useLineHighlights`. Thus saved live comments are part of the highlighter's
agent annotations, not merely a separate overlay painted afterward.

The current native `prepare_extension_line_highlights` filters the raw
`Changeset.files`, while saved comments live separately in `ReviewState`.
`merge_file_annotations_by_file_id` and `build_review_stream_state` exist, but
are not used at this invocation boundary. The next integration must project
saved comments into consumer-owned file views, preserve baseline annotations,
and invalidate highlighter derivations on note creation, edit, and removal.
It must not mutate the immutable review document or reacquire its already-held
state lock during rendering. Borrow unchanged files rather than cloning the
entire changeset merely to attach a note to one file.

Required evidence remains a compiled extension observing those note transitions
and refreshed terminal marks, plus checks for orphaned/draft notes and retained
source-reader authority. The existing integration pass is not that evidence,
and neither source file receives new ledger coverage from this audit.

## Published marks during a pending refresh

A held-worker regression reproduced marks disappearing immediately when an
epoch refresh began. Pinned Hunk retains its previous publication while the
same file and registrations prepare their replacement. The native coordinator
now retains that exact merged array during a pending refresh only while its
published paint inputs still match, ignoring epochs but not content, source,
reader generation, agent context, or registration identity.

The test releases the worker with an empty result and verifies that the old
marks then disappear. Negative cases replace content, source identity, reader
generation, or registration while the runtime is busy and verify immediate
removal instead of stale retention. This is publication-state evidence, not
whole-hook parity; the ledger remains unchanged.
Verification passes: all 39 highlighter tests, all 1,070 TUI unit tests, workspace
Clippy, formatting, and architecture checks.

## Merged highlight identity after empty refresh

A regression reproduced a merged array being replaced solely because an epoch
changed for a contributor that still returned no marks. The pinned hook compares
contributing part identities, not epoch labels, when deciding whether to reuse
its merged array. Native reuse now follows that rule without retaining an extra
epoch signature. The test verifies reuse after the empty contributor refreshes
and replacement when a real contributing array is rederived.

Pending-refresh publication remains a separate parity question; this change
proves settled merged-array identity only and adds no ledger coverage.
All 37 highlighter tests, workspace Clippy, formatting, and architecture checks
pass for this checkpoint.

## Unwinding highlighter worker failures

The whole worker invocation, including deferred source hydration, now contains
unwinding Rust panics as failed highlight derivations. Signal cleanup and terminal
completion delivery still run. A regression deliberately panics, waits for the
worker's own completion without polling deadlines, then verifies slot release,
no marks, one warning, and no retry. Before the change it timed out waiting for
the lost completion.

This covers unwinding panics at the worker boundary, not process aborts, native
crashes, or all extension isolation behavior. No whole-hook mapping is claimed.
All 36 highlighter tests, workspace Clippy, formatting, and architecture checks
pass. Strict audit still rejects the 313 unmapped records.

## Native highlighter request cleanup

The host now sends `$/cancelRequest` after decoding a highlighter response,
including an extension error, matching the pinned request's final child-signal
cleanup. The decoded result remains authoritative if cleanup delivery fails.
A strict subprocess fixture rejects its next invocation unless it received
cleanup for the previous request; the regression failed on its second call
before the host change and now covers successful and failed responses.

The example's hanging request now remains unresolved while its input loop keeps
servicing notifications. Timeout and supersession tests require a subsequent
successful invocation in strict mode, proving that cleanup reaches the child
rather than only observing that the host returned. The live Ratatui cell-buffer
example is retained. This closes the protocol gap noted below, not whole-hook
coverage; no ledger mapping is added.
Verification passes: all five executable highlighter tests, 13 host highlighter
tests, workspace Clippy, formatting, and architecture checks.

## Worker-completion signal cleanup

A regression receives the worker result without polling the coordinator and
checks its cancellation signal. Previously the signal remained live until UI
settlement. Workers now perform cleanup before sending a completion and record
whether cancellation preceded that cleanup. Settlement can therefore accept a
successful completed request while continuing to reject genuinely cancelled or
retired requests through the existing token-identity checks.

The test also returns the queued completion to the coordinator and verifies
that valid marks publish without warnings. This proves the in-process worker
boundary, not every native subprocess signal/lifecycle behavior. No whole-hook
mapping or additional ledger coverage is claimed.
The native host's cancellable RPC currently sends cancellation on timeout or
supersession, but returns a successful response without a cleanup notification;
that separate protocol boundary still needs lifecycle verification.
Verification passes: 35 highlighter tests, all 1,066 TUI unit tests, workspace
Clippy, formatting, and architecture checks.

## Highlighter warning text

Failure and timeout warnings now use the pinned hook's fixed attribution text,
without appending native exception or timeout details. A regression first
observed the extra `(boom)` suffix; exact assertions now cover a thrown failure,
a queued result past its deadline, and a still-blocked request expiring during
polling. Highlighter IDs use literal quoted interpolation rather than Rust debug
escaping across failure, validation, invalid-range, and merged-cap notices.

This verifies warning formatting at the coordinator boundary, not all native
extension diagnostics or whole-hook parity. No ledger mapping is added.
All 34 highlighter tests, workspace Clippy, formatting, and architecture checks
pass for this checkpoint.

## Identical-content reload highlight ownership

Pinned `AppHost.tsx` adopts a replacement bootstrap on successful reload, and
`useLineHighlights` requires the exact file object for cache reuse. A native
regression using `ReviewApp::reload` reproduced old marks surviving an identical
document reload. The successful reload commit now resets file-derived highlight
state and cancels unfinished requests before the next frame. It does not retire
the preparation owner, so the replacement document can prepare fresh marks.

The reset preserves registration-scoped warning history. It is placed inside
the committed-reload path, not the fallible preparation/publication path. The
test verifies immediate removal and a second invocation for identical content.
This closes that specific reload gap; it does not establish every collection
identity or lifecycle case and adds no ledger coverage.
Verification passes: 34 highlighter tests, all 1,065 TUI unit tests, workspace
Clippy, formatting, and architecture checks.

## Visible-file highlighter routing

Pinned `App.tsx` passes `review.visibleFiles` to the highlighter hook. The native
reviewer was instead passing its complete changeset. It now applies the same
review filter used by the canvas before submitting preparation work. The
coordinator accepts borrowed file iterators, avoiding a cloned filtered diff
tree while retaining its existing worker-owned copies.

A regression switches alpha → beta → alpha and then to no matches. Only matching
files invoke the highlighter, hidden marks disappear immediately, retired cache
entries are not reused when a file returns, and an empty result retires all
marks and cached derivations. This is filter-routing evidence, not a completed
whole-hook mapping. Same-content document replacement still requires explicit
identity/lifecycle verification; the ledger remains unchanged.
Verification passes: all 33 highlighter tests, all 1,064 TUI unit tests, workspace
Clippy, formatting, and architecture checks.

## Ineligible-file generation ownership

The preparation generation now retains a lightweight identity for every file,
including binary, oversized, and empty-diff files. Adding or replacing one of
these files retires the previous unfinished pass without invoking a highlighter
for the ineligible file. The regression first failed with a held worker observing
no cancellation; it now covers addition and replacement for all three kinds.
Unchanged polling still retains the original live request token.

This closes the ineligible-file gap noted in the preceding checkpoint. Equivalent
input collection identity and full hook lifecycle parity remain unproven; no
ledger mapping is added. The additional identity storage contains keys and flags,
not cloned diff trees, and remains subject to the release memory benchmark gate.
All 32 highlighter tests, workspace Clippy, formatting, and architecture checks
pass for this checkpoint.

## Preparation-pass supersession

A held-worker regression reproduced an unfinished request surviving the addition
of another review file without cancellation. The coordinator now compares the
ordered active task keys before settlement and cancels all unfinished requests
when that preparation pass changes. Completed cache entries are retained when
their individual keys still match. Unchanged polling retains the same request
token; an unchanged file with unfinished work restarts after supersession.

The generation comparison borrows existing keys on unchanged polls rather than
allocating another key vector each time. This models active preparation changes,
not every upstream React dependency identity: changes involving only ineligible
files or otherwise equivalent input collections still require separate audit.
No whole-hook mapping or additional ledger coverage is claimed.
Verification passes: all 31 highlighter tests, all 1,062 TUI unit tests, workspace
Clippy, formatting, and architecture checks.

## Whole-operation highlighter deadline

Preparation deadlines now start before worker launch, covering deferred source
hydration as well as the native extension request. Coordinator polling expires
overdue requests, signals cancellation, frees their slots, caches a failed
derivation, and reports the timeout once. Completion timestamps prevent a late
result queued before the next poll from bypassing the deadline. Successful
settlement also signals cancellation, matching the pinned request's cleanup.

Tests hold a worker beyond an injected deadline, verify cancellation and slot
release, then deliver its late marks and check that neither publication nor
restart occurs. A separate queued-result test checks the deadline boundary.
All 30 highlighter tests, workspace Clippy, formatting, and architecture checks
pass. Expiry is coordinator-driven; blocked source threads are not forcibly
terminated. This does not establish latency or memory parity, full generation
semantics, or whole-hook coverage. The ledger remains unchanged.

## Early registry-retirement cancellation

Native extension registry retirement now cancels pending highlighter preparation
before retiring child processes, rather than waiting for the preparation owner's
eventual drop. Retirement is terminal: later reconciliation cannot restart work
or publish results through that owner. A held-worker regression exercises the
actual registry-retirement entry point while keeping its owner alive; it failed
before this change and now observes cancellation and no restart.

All 28 highlighter tests and all 1,059 TUI unit tests pass, together with workspace
Clippy, formatting, and architecture checks. This is a lifecycle checkpoint, not
whole-hook parity: the highlighter hook remains unmapped and no ledger coverage
is added.

## Registration-ordered file preparation

The pinned highlighter hook prepares up to four files concurrently and awaits
each registration in order within a file. A controlled test reproduced the
native scheduler instead starting two registrations for each of the first two
files. Scheduling now gives each file's first unresolved registration its turn;
an existing worker or busy earlier native connection blocks later registrations
for that file without blocking independent files.

Tests verify the initial four requests belong to four files, complete per-file
registration ordering across five files, and a busy first extension that cannot
be overtaken by the second. Remaining whole-hook generation semantics still
require verification; this change adds no ledger coverage.
All 27 highlighter tests, workspace Clippy, formatting, and architecture checks
pass for this scheduler checkpoint.

## Highlighter disposal and completion ownership

A controlled regression reproduced a running highlighter observing no
cancellation after its preparation owner was dropped. Owner disposal now signals
all current cancellation flags without waiting on provider or extension code.
Another regression showed retired jobs retaining all four preparation slots;
retirement now removes them from the current owner's pending map immediately.

Completions carry their original cancellation token. Settlement checks token
identity as well as the task key, preventing an old success, failure, or retry
from consuming or publishing through a newer request that reused the same key.
Tests cover real worker disposal and controlled late-result delivery. All 25
highlighter tests pass. Remaining generation semantics
still need verification; the complete hook remains unmapped.
Workspace Clippy, formatting, and architecture checks also pass for this checkpoint.

## Indexed runtime source binding

Bulk provider installation uses files from the immutable review snapshot rather
than searching and cloning a complete file for each handle. Reopening gaps also
uses those known files, with a deduplicated expanded-file index. Single-file
actions borrow from an owned review snapshot and leave the worker to make its
necessary captured copy.

Reload retirement indexes attested `(file key, source identity)` pairs once per
reconciliation instead of scanning every file for every binding or presentation
entry. This preserves the previous membership rule, including duplicate-key
inputs, while removing the nested scans. A 256-file test verifies binding,
unchanged rebinding, retirement, zero source reads, and unchanged document identity.
These structural changes are not evidence that the release latency or peak-memory
benchmark gates pass. No ledger coverage is added.
Verification passes: 51 source-related unit tests, six source-filtered terminal
target tests, workspace Clippy, formatting, and architecture checks.

## Transform-owned deferred source handoff

The extension host carries reader handles through the same opaque-metadata
validation that identifies each transformed file's original renderer file.
Reordering, filtering, display-path changes, and composed transforms produce a
replacement registry only after the complete transform validates. Rebinding
does not read source, construct a reader from metadata, or change the provider's
captured request. Invalid output leaves the previous registry unchanged; filtered
files lose bindings from the replacement while retained generations keep theirs.

CLI startup and dynamic session reload pass this registry through language
overrides and transforms before constructing their publication source reader.
Tests cover composition, cached provider reads using the original path, rejected
forged metadata, and an empty registry that cannot gain authority. A real-Git
PTY fixture changes the displayed path to a nonexistent file and verifies source
expansion and a changed-input reload in both layouts. Identical attested Git
inputs deliberately reuse source and are not treated as fresh-read evidence.

Checkpoint checks pass: 92 terminal tests, 91 CLI unit tests, 1,052 TUI unit
tests, five host transform tests, four provider-capability tests, workspace
Clippy, formatting, and architecture checks. The terminal test explicitly
reopens the gap after the changed-input reload collapses it.

This handoff does not complete the remaining source/review hook contracts or
increase ledger coverage. Legacy snapshot-only reload entrypoints remain separate
from the dynamic VCS path.

## Deferred native line-highlighter sources

Highlighter workers capture the current provider registry and obtain a
consumer-owned source copy before invoking native extension code. Registration
does not read sources; cancellation is checked before and after provider reads.
The original review document remains unchanged and resolved sides use the same
provider cache as other source consumers.

A regression reproduced stale highlight reuse after source identity changed
while patch content stayed unchanged: the extension ran once instead of twice.
Source identity is now part of both scheduling and publication cache keys, so
old-source completions cannot satisfy new-source tasks.

A second regression reproduced reuse after replacing an unattested source
handle with unchanged patch/source identity. Provider handles now have a
process-local, non-serialized identity. Unattested highlighter task keys include
that identity; matching attested source keys still reuse results. A controlled
worker test releases the retired reader's result only after its replacement has
published, and verifies that the late result cannot replace current highlights.

All 19 highlighter tests and three provider-capability tests pass, along with
workspace Clippy, formatting, and architecture checks. The full workspace run
started at `d510fd07` also completed successfully (including 190 xtask tests,
one ignored); the focused tests above separately verify these later changes.

Fresh highlighter registrations now receive process-local identities that are
stable across clones and distinct even when public extension/highlighter names
match. Both preparation/publication keys and warning deduplication use those
identities. A regression reproduced a same-name replacement receiving its
predecessor's cached result; tests now verify fresh execution and independent
warning reporting while repeated renders of the same registration stay cached.
All 21 preparation tests and 13 host highlighter tests pass, together with
workspace Clippy, formatting, and architecture checks for this registration change.

Agent context is a separate highlighter input: it is intentionally excluded from
diff content identity but included in the extension file projection. A regression
reproduced stale highlights after adding agent rationale to an unchanged diff.
Scheduling and publication now include a digest of that context. Tests verify
addition and removal rederive marks while unchanged clones reuse them, without
changing the underlying diff identity or loading source to compute the key.
The complete TUI unit suite passes at this checkpoint (1,052 tests), as do
workspace Clippy, formatting, and architecture checks.

This does not establish complete cache parity: the remaining whole-hook contracts
still need explicit review. The previously
mapped full `useLineHighlights.ts` interval is
reopened as unmapped. Its prior destination/evidence references are preserved in
`highlighter-work-in-progress.json`, not as a completed ledger mapping.
The honest unmapped count is now 313, not 312.

## Deferred interactive VCS inputs

Interactive diff, show, stash, and dynamic VCS reloads now materialize patch
metadata and retained provider capabilities without reading full tracked-file
sources. Expansion and session resource requests read those capabilities on demand.
The eager compatibility API remains available to snapshot consumers. Untracked
diff synthesis still reads its input; direct-file and patch inputs retain their
existing behavior. Native bootstrap/working-tree benchmarks use the deferred path;
previous timing results are not evidence for its performance.

A counting-provider test verifies zero initial source reads and independent,
cached reads of each side. A real-Git PTY test in both layouts edits an unchanged
line after the initial frame with watch disabled, expands it, and observes the
new text. It also opens a note and verifies that viewing creates no repository
state. The session reload test now uses actual deferred materialization rather
than clearing eager snapshots.

The first full terminal run exposed four file-view regressions: those consumers
had relied on eager snapshots. Matched file-view workers and invoked extension
workspace contexts now obtain consumer-owned source copies through retained
provider handles. Workspace writes use the same cached provider snapshots for
origin/attestation and optimistic disk-content checks. The review document is not
mutated; retirement removes handles from future consumers while already captured
generations retain their own registry. Highlighter workers now use those sources
as described above; source preservation through metadata-changing transforms
still requires follow-up.

Checkpoint verification: all 91 terminal tests and 1,045 TUI unit tests pass,
along with the focused source-copy test, workspace Clippy, formatting, and
architecture checks. The earlier full-workspace run stopped at the file-view
regressions; it is not recorded as passing. Strict audit still rejects the 312
unmapped records, and the cached upstream queue still contains 11 commits.

This does not complete the source hook or change
ledger coverage; full annotation/session and remaining input parity is unfinished.

## Runtime-loaded cursor selection

A live regression reproduced a panic when selecting an asynchronously rendered
source line: selection validation consulted only provider snapshots. Runtime
source selection now checks the current source identity, hunk/file bounds, and
the loaded text's line bounds without modifying the immutable changeset. Ordinary
diff-line and snapshot-backed selection retain their existing contracts.

An expansion retains its pending cursor reveal until the requested rows exist.
Completion selects the first requested source row; collapsing before completion
cancels that reveal, while collapse after selection restores the earlier cursor.
Attested runtime text can preserve its selected line across a matching reload;
changed identities retire the text and selection. Channel-controlled live tests
exercise these transitions, including the original failing cursor path.

Remaining input and annotation/session validation work is still required.
This does not complete the entire Hunk hook or add ledger coverage.

## Generation-owned session source readers

Each prepared publication owns its source reader and resource store. A successful
reservation commit advances publication, resources, and reader together; cancellation
does not replace current source authority. Future preparations inherit the committed
reader, while retained old stores keep their original reader even if first read later.

Initial interactive sessions use retained VCS capabilities for source resources.
Dynamic reload prepares replacement source authority before broker registration and
installs it only through the publication commit gate. Legacy snapshot-only reloads
explicitly select the snapshot reader, preventing an old VCS cache from overriding
fresh snapshots. Missing executable handles may use immutable snapshot values but
cannot open files or invoke a provider.

Tests cover cancelled/committed reader replacement, old-generation isolation, and
initial/reloaded source resources through the live session adapter, including failed
registration. Interactive VCS reads are now deferred as described above;
remaining input and loaded-line validation work is unfinished. Ledger coverage is unchanged.

## Live source loading presentation

The real stack/split gap renderer consumes identity-bound pending, loading,
loaded, unavailable, and too-large state. It preserves the immutable changeset:
worker text does not become a fabricated provider snapshot. Pending/error rows
remain addressable, and loaded text supplies expanded rows and syntax highlighting.

`ReviewApp::install_source_loader` installs explicit host-owned authority for a
mounted file. Gap toggles start the request owner; standalone and embedded loops
poll completions on the UI thread. Reload retires changed/unattested bindings,
and app reset retires all bindings. Tests use blocked workers to verify loading
and completion in the live row builder and rejection of stale reload results.

Initial CLI VCS reviews now pass retained handles through the composition-root
bootstrap into the mounted app. Dynamic reloads carry replacement handles through
the publication transaction and install them only after a successful commit.
Replacing unattested handles refetches open gaps even when diff/source identity
is unchanged; a test exercises the real constructor and dynamic reload gate.
Absent replacement authority removes obsolete presentation and executable bindings.

Direct-file/patch inputs still use their existing snapshot behavior. Asynchronous
cursor reveal/restoration is integrated, but complete loaded-line session semantics
and remaining reload/input paths still require parity work. This integration does not complete the Hunk hook
or increase ledger coverage.

## Asynchronous source request owner

`workdeck-review::ReviewSourceRequests` provides the worker/completion boundary
for source loads. It skips loading/loaded state, allows retries after failures,
and checks request ID, side, and loader identity before settling a result.
Retirement removes authority without waiting on a provider; late successes are
ignored and late failures retain diagnostics. Typed size failures preserve their
distinct state. Workers cannot mutate the review store directly.

Deterministic channel-controlled tests cover these transitions. The live canvas
now uses this request owner and pending cursor reveal as described above; remaining
input/reload paths still require integration. No source interval gains coverage
from this component alone.

## Runtime provider source ownership

`LoadedVcsChangeset` retains a non-serialized source-capability registry, bound
after final review addresses and source identities are established. Each handle
captures its provider request, keeps old/new resolved results independently, and
retains snapshot origin/attestation and typed size failures. Ordinary failures
remain retryable. Duplicate file paths do not share a per-file cache.

Eager materialization uses these same handles for its initial reads; subsequent
access reuses those resolved results. Interactive materialization defers those reads.
Unknown or retired identities do not resolve a handle, and supplied metadata cannot
change the captured provider request. The initial/dynamic VCS handoff now reaches the
asynchronous controller, but full input and cursor/session parity remains
unfinished; this change does not increase ledger coverage.

## Source capability identity integration

The shared `DiffFile` model now retains optional source-capability identity
metadata separately from loaded source snapshots. Frozen document projections
from both pinned Hunk trees cover absent capabilities, absent and empty cache
keys, changed keys, runtime IDs, content, and paths. Core tests reproduce those
identities and attestations before loading and after replacing source text.
Legacy serialized files omit the new optional field and remain readable.

VCS materialization preserves the provider cache key instead of substituting a
loaded text digest. The descriptor is data, not executable authority: reading it
does not grant filesystem access. Provider fetcher handoff, asynchronous loading,
retry, and reload retirement in the live review controller remain unfinished.
This change adds no ledger coverage. See
`oracles/source-capability-identity.json` for the pinned driver and raw outputs.

## Repaired filesystem fetcher coverage

The mappings for `src/core/changeset/fileSource.ts` and its complete test file
were reopened as unmapped in `3acf785b`. Their previous destinations implemented bounded
reads and snapshot storage, but not the source module's per-fetcher old/new cache
of resolved text and missing results. In particular, the upstream test that
rewrites a file after its first fetch has no corresponding cached-fetcher test in
those destinations. A broad source/test file link was insufficient evidence.

The native `workdeck-vcs::FileSourceFetcher` contract and its filesystem
implementation now provide both source specs, the default/configurable limit,
typed limit errors, an absent attestation key, and independent caches for old/new
resolved text or missing values. Errors are not cached. The cache mutex is not
held during I/O, and in-flight requests are not cached. Host scheduling is separate
from this synchronous, thread-safe source capability.

All five source tests now have explicit Rust test anchors, plus a native regression
for cached missing values and retryable size failures. Both pinned five-test files
were executed successfully; `oracles/file-source-execution.json` retains the raw
receipts and test mappings. The full 236-test VCS suite and VCS Clippy pass.

The two intervals regain coverage only with this implementation and executable
evidence. Their original boundaries, blob hashes, classification, and provenance
are unchanged; Git retains the withdrawn mappings. The provider-to-live-review
lazy-load integration remains a separate unfinished responsibility. These library
tests do not certify that live controller integration.

## Source decoding parity

`oracles/source-text-decoding.json` freezes eight file/stream cases from both
pinned source trees under Bun 1.3.14, including raw outputs and the oracle driver.
The downloaded runtime archive was checked against its published SHA-256.
The differential Rust test failed on a leading BOM before the fix. Native source
readers now consume one initial UTF-8 BOM, preserve embedded BOMs, and enforce
the input-byte ceiling before decoding. Invalid UTF-8, split emoji bytes,
truncated input, empty input, and NUL are checked against both pins. Stream tests
retain the recorded chunk boundaries instead of flattening them before reading.
This corrects existing mapped source behavior; it adds no interval coverage.

## Filesystem source-read ceiling

Filesystem source reads now open one handle, inspect that handle's metadata, and
use the bounded stream reader. Metadata can reject an oversized source early but
cannot authorize an unbounded allocation if the file grows afterward. The byte
ceiling bounds accumulated input, not UTF-8 replacement output or the fixed read
buffer. Interrupted reads retry without discarding already-read bytes.

VCS tests cover stale size hints with an unending reader that fails if consumption
exceeds one fixed read buffer beyond the limit, early rejection without reading,
exact limits, empty files, invalid UTF-8, and interrupted I/O. This hardens the
existing source reader without adding ledger coverage or implementing the still
missing live lazy-source-fetch integration.

## Native note-target lookup diagnostic

The live stream now accumulates note targets in row order before constructing its
ordered lookup once. It retains complete offscreen targets and cursor geometry;
the existing full/viewport geometry comparisons and the complete TUI suite pass.
This changes allocation strategy, not visible-row or note ownership semantics.

`benchmarks/note-target-lookup-native-ab-7e95782d.json` records three alternating
release-binary pairs with the exact source patch, binary hashes, and raw output.
Aggregate median scroll latency was 3.656083 ms unchanged versus 3.211708 ms with
bulk construction. Maximum measured process peak RSS was 167,608,320 versus
168,951,808 bytes: this is not a memory improvement. The temporary target vector
coexists with the final lookup during construction. These are native-only
diagnostics, not a fresh Hunk comparison or a benchmark-gate pass. No interval is
newly mapped by this optimization.

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

Before a fetch can prune or replace tracking refs, `cargo xtask port fetch` archives
the existing branch tips, exact tag objects, and pinned anchors under
`refs/upstream/hunk/archive/{heads,tags,anchors}/<hex-name>/<object-id>`. It archives
newly observed refs after the fetch too, including successful updates from a partially
failed fetch. Hex-encoded names avoid ref file/directory collisions after branch renames.
Each archive write is an atomic, create-only Git transaction; existing archive refs
are verified, never moved or deleted by the tooling. A shared Git-directory lock
serializes these operations across linked worktrees. Fetch does not auto-create
unnamespaced tags and does not merge upstream ancestry into Workdeck.

`cargo xtask port preserve-upstream` archives already available local refs without
network access. `port/hunk/upstream-refs.jsonl` records their original ref names and
object IDs, making older deleted archive refs detectable even after upstream deletes
the corresponding branch. `cargo xtask port audit-upstream` is a read-only check of
receipt integrity, immutable ref identity, and coverage of current tracking refs;
strict port audit also requires it. It rejects corruption rather than silently repairing
the receipt. These are local tooling guards, not hosted branch-protection settings:
an owner can still change Git refs manually, which the receipt audit must detect.
Commit receipt updates with port work and retain the archive refs when moving repositories.
Temporary-repository tests cover force pushes, branch deletion/name-prefix reuse,
annotated tags, symbolic-ref rejection, missing history, partial fetch failure, writer
exclusion, idempotence, and history reachability after garbage collection.
The initial local preservation archived 612 refs; a repeated run added zero and the
read-only archive audit verified all 612 receipts. All seven archive regressions pass;
the full xtask suite passes 198 tests with one existing opt-in capture test ignored.
This does not constitute the required final upstream fetch or clear the catch-up queue.

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
The synchronous native PTY probe accepts parent cancellation notifications without attempting to
decode them as requests with IDs. This includes cleanup after successful highlighting; otherwise
the probe exits before refresh or a queued command can run. The existing refresh and held-highlight
command tests both reproduce the regression without this handling and pass with it; all nineteen
native extension PTY cases pass together. This fixture repair adds no source-ledger coverage.
The native trust fixture commits its compiled extension and manifest before changing the two
source files, matching the source harness's tracked-extension baseline. Its alpha/beta contents
now match that factory rather than borrowing the distinct layout fixture. A shared repository
factory verifies committed pre-change bytes, retained prepared entries, exact changed paths,
source author/message metadata, and owned temporary-directory cleanup. No compiled fixture is
added to Workdeck's own tracked tree; the binary lives only in a disposable test repository.

The ordinary compiled line-highlighter now uses the SDK's `ExtensionDocumentCallbacks` router
instead of nesting a synchronous response wait inside its request loop. A four-file regression
holds each captured source until all four reads have started, with the batch fixture mode disabled.
It failed with unexpected-response errors and timeouts before the change and passes with the
router; all eighteen compiled line-highlighter integration tests pass together. Four SDK tests
cover callback attribution, retirement, malformed responses, limits, and ID exhaustion. The router
does not own framing or transport deadlines and does not complete the unmapped highlighter hook.

A separate PTY regression deliberately exits the highlighter child and keeps the review open beyond
the highlighter deadline. It reproduced an application shutdown from a broken extension pipe on
macOS. Darwin's per-descriptor `F_SETNOSIGPIPE` fixes that path; thread masking alone did not.
All twenty extension PTY cases, nine terminal lifecycle cases (including explicit SIGPIPE), and
185 host unit tests pass with the fix. The process-wide signal handler is unchanged. The other
Unix write path uses a scoped mask, with restoration tested locally; its Linux-native validation
remains outstanding. This adds no source-ledger coverage or cross-platform release claim.

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

The native broker listener explicitly makes each accepted socket blocking before applying its
existing read/write timeouts. Darwin can inherit the listener's nonblocking mode; previously a
connection accepted before its first HTTP bytes arrived was dropped on `WouldBlock`. A delayed
WebSocket-header regression reproduces the failure without retries and passes after the correction,
alongside all twenty-four adapter tests. Existing message ceilings, admission/pressure checks, and
timeouts remain unchanged. This repairs an already mapped transport path without adding coverage.

### Cross-consumer review conformance (in progress)

`cargo xtask port capture-review-conformance /absolute/path/to/bun` explicitly requires
Bun 1.3.14 for disposable oracle execution only. The Rust command archives each exact pin
into a temporary directory, installs its locked dependencies with lifecycle scripts disabled,
runs its original conformance test file, and captures real geometry, navigation, snapshot,
and event consumer projections. Each pin gets isolated configuration. No upstream source
checkout or runtime dependency is retained in Workdeck. Both suites and capture validations
must succeed before the command writes either tracked fixture.

`oracles/review-conformance-main.json` and `oracles/review-conformance-stable.json` retain
separate outputs because the pins differ. The source suites passed 111 and 106 tests,
respectively, on macOS arm64 with Bun 1.3.14. These source-suite passes are not Rust parity.
The capture includes publication ordering and producer lifecycle transitions, including
their independent input addresses and step sequences. It also captures every wire action,
note-body draft transition, and whole-note size verdict. Size fixtures retain exact UTF-8
serialized byte measurements without storing large repeated filler strings.

`cargo test -p workdeck-cli --test review_conformance` independently parses all six geometry
inputs in Rust and checks core gaps, inclusive hunk ranges, default note targets, expansion
text, and binary-rename labeling against both captured core consumers and expectations.
The same corpus also exercises the actual terminal row planner, selected-hunk summaries,
live-comment targeting, empty-file messaging, and producer publication/expansion intents.
Producer canonical resources are checked against their published manifests. This exposed
and fixed a canonical-projection bug: an empty side of a unified hunk positions the hunk
after an unchanged line, which must be counted in the leading gap. The former producer
manifest incorrectly omitted line 1 for the insertion and deletion cases despite correct
renderer geometry. Direct core regressions also cover zero-count ranges at the file start.

The navigation module independently builds the five pinned fixtures and drives both the
intent planner and the terminal reconciliation/store path. It checks all requested moves,
annotation scopes, clamping/wrapping, hidden/vanished/absent selections, and reveal targets.
The terminal adapter reads reveal counters from the real store transition; it does not
reuse the planner's returned reveal as its answer. Both consumers pass both pinned corpora.
The ordering module drives all ten publication cases through both the shared classifier
and a real broker mirror seeded with the current catalog/address. It also applies both
producer lifecycle sequences, reattaching the new generation's store after each reload.
Both pins pass, including revision jumps, replayed and retired positions, foreign producers,
malformed identities, and successive reloads. The geometry, navigation, and ordering fixture
source files are mapped only after these complete executable translations pass.
Individual consumer source files remain unmapped until their adapters are separately reviewed.

The wire module compares all 22 main and 17 stable actions through the strict native parser,
JSON intent projection, and typed lowering. Note tests execute all eight body policies and
their draft save/cancel actions, plus six whole-note byte-boundary cases at both pins.
The snapshot test exercises the actual extension projection while a real terminal composer
displays unsaved text. It retains the three-note stable input separately from main's four-note
input with its saved reply, preserves stale/orphaned anchors and collection order, and checks
revision and file identity exactly. Those four complete fixture files are now mapped.

The event module now drives all four window fixtures through both the shared protocol
and an authenticated, real loopback HTTP listener. It parses the streamed SSE frames,
reassembles chunked publications with the native assembler, and requires byte-exact
round trips of the native serialized publication. Exact-window and one-byte-over-window
boundaries, resumable frame counts, and adjacent-only frame-name collapsing are checked
against both pins. The event fixture and framing-helper source files are now mapped.
The 22 Rust test functions execute the pinned corpus cases through registered native
consumer callbacks; registry and finding-coverage assertions guard accidental omissions.
Core geometry first projects the canonical review file, then uses canonical gap selectors
and content-manifest geometry, rather than observing only the parser's DiffFile.
Focused conformance tests, formatting, and focused Clippy pass for this increment.
The complete harness and registry are now mapped to all participating Rust modules and
their executable test entrypoints. The protocol-event, HTTP-event, and wire consumers
are also mapped after adapter review. Additional reader tests reject missing, duplicate,
malformed, corrupt, and mismatched frames and check byte-fragmented SSE input stopping
at the resumable event boundary. The other individual consumer mappings remain under review.
The core-ordering and broker-mirror consumers are now separately reviewed and mapped:
the former invokes the shared ordering classifier and the latter seeds a real mirror
with its resource catalog before classifying the observed update. The extension-snapshot
consumer is also mapped to its public projection, preserving generation, revision, file
identities, saved-note order, reply links, resolution, and all exported anchor fields.
The intent-planner and terminal-navigation consumers are now mapped after completing
their positional helper branches. Invalid file positions become vanished selections;
annotation indices outside the document are ignored; duplicate annotations are sets;
and an explicit annotated-file list (including an empty list) is independent of hunk
annotations. Regression tests exercise that independent scope through both real consumers.
All 16 focused conformance tests pass. The full workspace test phase and workspace Clippy
also pass; the workspace test binary predates the final helper increment, which is covered
by the separate focused run. The release build and large-repository smoke then passed,
completing `cargo xtask verify`. During the full run, native watcher tests exceeded 60
seconds but all 241 VCS tests finished successfully in 91.65 seconds; no watcher change
or timeout adjustment was made. These results do not clear strict source coverage,
upstream catch-up, performance, cross-platform, installer, signing, or release gates.
The core-model and producer geometry consumers are now separately mapped after completing
their missing-gap, short-source, and multi-file helper paths. A missing expansion target
omits expanded rows; unavailable source rows retain their labels with empty text. The
producer adapter publishes the whole file stream together and checks every canonical
file against its manifest. Additional tests cover empty streams, expansion on a later
file, and out-of-range file/gap targets. All 18 focused tests and focused Clippy pass;
this newest test-helper increment was not part of the earlier full verification run.
The terminal geometry consumer is now mapped after its separate adapter review. Its
multi-file path drives the native row planner with explicit hunk headers, scopes expansion
to the requested file, and preserves its source-specific empty `expandedRows` array for a
missing gap (core and producer omit that field instead). Empty streams and out-of-range
file targets are also covered. All 19 focused tests and focused Clippy pass. The shared
conformance type definitions remain unmapped pending their complete Rust translation.
Their geometry, navigation, saved-note snapshot, and event-framing output models now
have executable typed Rust checks in `review_conformance/models.rs`. Each native output
and pinned expectation must survive exact JSON-value round trips through the model;
unknown fields, malformed ranges, invalid variants, and missing required null fields
cannot disappear during comparison. All 21 focused tests pass. Fixture interfaces and
consumer signatures are still incomplete, so the entire source types record remains
unmapped; these checks do not substitute for its remaining translation.
All six consumer registries now use named Rust descriptors containing their original
registration phase and executable callbacks. Navigation registers callable adapters
rather than dispatch flags, and wire parsing and note acceptance have named callback
fields. The registry test verifies each phase as well as each name; phases describe the
upstream harness history, not Workdeck's native SDK version. All 21 focused tests pass.
Fixture interfaces and fixture-bound signatures remain unfinished, so the types record
is still unmapped and the source-coverage count is unchanged.
Geometry now uses a fixture-bound callable signature returning its typed projection.
Its fixture retains the source ID, finding IDs, original adversarial description, real
builder, file-scoped expansion request, and hand-written expected projection. All three
geometry consumers use that interface in both pinned corpus runs. An additional test
counts one builder invocation per registered consumer and checks expansion on the second
file against the composed pinned expectations. All 22 focused tests pass. The other
fixture families still need their complete interfaces; the types record remains unmapped.
Capture-integrity tests reject changed outputs and wrong pins but are not parity proof.

The subsequent interface increments (`ef0d87dd`, `8ab2035b`, `c06ce9b2`, `7e1bf0f6`,
`49006015`) bind event, snapshot, wire, and navigation fixtures to typed callbacks.
Event consumers receive the publication and relative/absolute window rule; snapshot
consumers receive a generation and fresh state builder; navigation consumers receive
fresh file builders, optional filtering/annotation scopes, and positional inputs.
All retain source metadata and hand-written expectations alongside raw oracle comparisons.
Ordering callbacks return the native publication-order enum. Builder-invocation tests
exercise both navigation consumers and alternate snapshot generations.

The interface review found an additional wire-size distinction: the native domain note
model omits empty tags on serialization, while the source wire shape can explicitly carry
`tags: []`. The conformance note adapter now preserves that presence and rejects fields
or nulls that typing would lose. A new executable boundary test proves that adding empty
tags to an exactly-at-limit note changes acceptance to rejection. This is a conformance
adapter correction, not a claim that every runtime note transport has been audited.
All 25 focused conformance tests pass after this correction. These latest increments
have focused validation only; they are not a fresh full-workspace or release-gate result.
The shared types record remains unmapped until its complete contract review is finished.
That review also separated conformance intents from the broader wire-action model:
expanded-line proofs and create-note target preconditions cannot appear in an intent,
and create/update consumption must be literal `true`. The typed expectation model now
enforces those distinctions, with an executable rejection/round-trip test. Both pinned
corpora still pass unchanged; the focused suite now contains 26 passing tests.

#### Shared conformance interface review

The complete pinned `test/review-conformance/types.ts` record is now translated. This
mapping covers its harness interfaces, not completion of the product or its imported
runtime subsystems. The source's two rules remain enforced: expectations come from the
hand-written upstream contract, and compared projections contain no renderer-specific
widths, cells, or DOM. Frozen adapter output is a second comparison, never a replacement
for those expectations.

| Source interface family | Rust implementation and executable checks |
| --- | --- |
| Gap, line address, hunk ranges, expanded row, file and geometry projection | `models.rs` typed lossless projection checks; all three geometry consumers and malformed-shape tests |
| Expansion, geometry fixture and consumer | Root conformance fixture builders, file-scoped expansion and fresh-builder tests |
| Snapshot projection, fixture and consumer | `models.rs` and `snapshot.rs`; saved notes, reply identity, unsaved terminal draft exclusion, alternate generations and fresh builders |
| Selection input/output, reveal, move/outcome, navigation projection | `models.rs` and `navigation.rs`; vanished/null positions, typed output, real planner and terminal store paths |
| Navigation fixture and consumer | Optional filter, keyed annotated-hunk map, independent annotated files, moves and selections; pinned corpora and explicit/empty/absent scope tests |
| Ordering consumer | Native publication-address inputs and publication-order enum results; core and real broker-mirror corpus paths |
| Wire outcome, fixture and consumer | Typed intent wrapper excludes wire-only fields, object action inputs preserve malformed source cases, note adapter retains explicit empty tags; both pinned wire corpora and size boundaries |
| Event projection, fixture and consumer | Typed publication body, numeric/relative windows and frame summaries; protocol and real HTTP consumers, boundary windows and corrupt/incomplete event checks |

All fixture families retain IDs, finding IDs, descriptions, and hand-written expected
projections. Generic named consumer descriptors retain registration phases and executable
family-specific callbacks; phase strings are upstream history, not SDK versions. Rust
builder closures replace source functions and build fresh native inputs. The source event
Promise becomes a blocking completion boundary in this synchronous native harness: the
real HTTP adapter reads through the resumable frame, closes its HTTP surface, and stops
its test daemon before returning the typed projection. It does not return pending work
or substitute a frame-list mock. Domain line/index integer types replace JavaScript
numbers; renderer geometry is never normalized away in the comparisons.

The complete focused suite has 27 passing tests after annotation-scope review, with
focused Clippy, formatting and diff checks passing. Earlier paragraphs record incremental
unmapped status; this review supersedes that status for this source record only.

Full `cargo xtask verify` passed at `e4f3c782` before this conformance increment, including
workspace tests, Clippy, release build, and the large-repository smoke test. This does not
clear strict source coverage, benchmark, native-platform, signing, or release gates.

A fresh `cargo xtask verify` run at `22f88cea` also passed after the complete shared
conformance-interface mapping and upstream-ref archive tooling were committed. This run
passed workspace tests (including all 27 conformance tests, 241 VCS tests, and 198 xtask
tests with one existing ignored opt-in test), workspace Clippy, release build, and the
large-repository smoke check. The intentional temporary-repository fetch-lock error in
the archive failure test was followed by a passing test; it was not a checkout lock.
Strict `cargo xtask port audit` still fails with 290 unmapped records and 11 cached
upstream commits pending. This verification does not satisfy the benchmark, final fetch,
native platform, or release-signing gates and does not remove the legacy per-TUI session
listener that still coexists with the native broker.
The later full-workspace run at `fc471e27` failed ten app-host workspace tests during
extension-probe startup with handshake timeouts; its geometry conformance tests passed.
That failure is under investigation and the run must not be reported as a workspace pass.
An isolated rerun of all eleven app-host workspace tests passed without changing their
deadlines or implementation. This does not establish the cause of the earlier timeout.
Three runs of the exact earlier test executable also passed. A fresh full-workspace run at
`2dc01b5d`, without concurrent builds or oracle execution, then passed, with one existing
opt-in oracle capture test still ignored. The timeout is not claimed fixed or causally explained.

### Native quit transport and caller policy

The native broker now parses and dispatches `quit_session` and the HTTP `quit` action.
The mounted review waits for the matching result to enter the producer socket queue
before requesting shutdown; queue notification is not a peer acknowledgement. Timeout
or an abandoned request leaves the review open. A real authenticated loopback test,
`authenticated_native_quit_reply_survives_immediate_producer_disconnect`, verifies that
the HTTP client receives the successful result even when the producer stops immediately
after queuing it, and that the session subsequently disappears from discovery.

The expanded local caller policy uses `security-v1/caller-v2.json` and grant/revocation ID
`workdeck-caller-bootstrap-v2`. Its only new command is `quit_session` version 1. The
stored credential format, daemon identity and producer identity remain unchanged.
Existing `caller.json` is neither changed nor imported with broader privileges. New
identity publication retains owner-private permissions and atomic first-writer adoption;
subsequent loads reuse it. Exact command and operation validation remains mandatory.
An older running daemon must be restarted before it can recognize the new caller key.
Tests cover byte-for-byte preservation of old credentials, new identity reuse, and
rejection of added, missing or version-altered command scopes.

All 571 session library tests pass after this increment. This is transport and policy
evidence, not a completed CLI or TUI end-to-end migration: `workdeck session quit` still
uses the legacy listener at this point, and the remaining legacy routes and listener
must be removed only after their native replacements have executable parity evidence.
No source ledger interval or upstream catch-up commit is marked complete by this work.

The subsequent CLI increment replaces the legacy `workdeck session quit` route with
`SessionCommandRunner::quit` and the authenticated native client. It checks daemon
availability and advertised quit support before dispatch, normalizes the selected
repository, and preserves the existing `live_session` / `quit` JSON envelope and text
result. Incompatible or unauthenticated older daemons return upgrade guidance rather
than falling back to the legacy listener. The real terminal-pager attention test now
ends by issuing CLI quit with the native ID returned by `session list`, draining
terminal restoration output, and asserting successful reviewer and CLI exits plus
the unchanged JSON reply. It sends neither keyboard quit nor a legacy request.
That PTY test and all 572 session library tests pass. Other legacy routes and the
per-TUI listener still remain; this does not claim the complete listener migration.
Workspace all-target Clippy and formatting passed for this increment, as did CLI
malformed-selector tests including missing and conflicting quit selectors. A fresh
strict audit still fails at 290 unmapped records with 11 cached upstream commits.

Full `cargo xtask verify` at `245d27ba` did not pass. Architecture, theme, skill and
release-fragment checks passed, followed by CLI integration, 27 conformance and nine
terminal-lifecycle tests. The terminal-pager suite ended with 56 passes and 37 failures:
the recorded failures report `No space left on device` while creating temporary
repositories, session runtime files, and nested Cargo query-cache/object outputs.
The verifier stopped at workspace tests, before its Clippy, release-build and smoke
stages. Earlier focused test and standalone Clippy passes do not make this run green.
Storage recovery and a fresh verification run are required.

After Cargo's package-scoped cleanup removed 23.8 GiB of regenerable `workdeck-tui`
build artifacts, `CARGO_INCREMENTAL=0 cargo xtask verify` passed at `c10ab4e8`.
That fresh run passed full workspace tests, including all 93 terminal-pager tests,
27 conformance tests, 572 session tests, 1,074 TUI tests, 241 VCS tests and 198 tooling
tests with one existing opt-in test ignored. Workspace all-target Clippy, the release
executable build and the large-repository smoke check also passed. Sources, fixtures,
Git history and Workdeck state were not removed during storage recovery. This supersedes
the storage-blocked verification result only; it does not clear strict ledger coverage,
upstream catch-up, benchmark, native-platform, signing or other release gates.

### Geometry-memory frozen source checks

`oracles/geometry-memory.json` retains eight actual source executions for each
pinned main/stable tree, including help ordering, invalid values, repeated numeric
options and the two-file workload with the full 50,000-line giant fixture. Source
blob verification and runtime provenance are recorded in the fixture. Output is
combined stdout/stderr, not a claim of independently captured streams.

`geometry_options_and_rows_match_both_frozen_source_oracles` executes native
geometry and checks seven deterministic metrics against both source captures:
114 ordinary body/bounds/materialized rows and 44,009 giant materialized rows.
It also checks parser outcomes and error messages for every captured case.
The focused test and xtask all-target Clippy passed. Timings, GC, JavaScript heap
statistics, help formatting and process exit behavior are not covered by this
test; this partial benchmark port remains unmapped.

### Linux native memory sampling (execution pending)

Native diagnostic snapshots now have a Linux backend reading current resident
bytes from `/proc/self/smaps_rollup`. The parser requires exactly one `Rss` field,
the kernel's `kB` unit, and checked conversion to bytes. It rejects missing,
duplicate, malformed and overflowing measurements. Unavailable allocator-zone
usage is serialized as `null`, never as a fabricated zero or JavaScript heap
equivalent. macOS retains its existing numeric allocator measurement.
See the [kernel proc documentation](https://docs.kernel.org/filesystems/proc.html)
for the distinction between page-table accounting and asynchronous RSS counters.

The parser and live macOS snapshot tests passed, along with xtask all-target
Clippy and formatting. A Linux-only live test is included, but native Linux
execution has not been performed: this host has no installed Linux Rust target
and its default Docker socket is unavailable. This is implementation progress,
not cross-platform or memory benchmark acceptance evidence. Windows remains
unsupported by this diagnostic backend; all related ledger records remain open.

### Windows native memory sampling (execution pending)

The subsequent Windows backend uses `K32GetProcessMemoryInfo` on the current
process pseudo-handle, returning `WorkingSetSize` and propagating API failures.
It does not substitute peak working set or commit charge for current resident
memory, and allocator-zone usage remains explicitly `null`. The target-specific
dependency reuses the existing locked `windows-sys` 0.61.2 package.

The actual source module and its tests passed `cargo check --target
x86_64-pc-windows-gnu --tests` in a disposable minimal harness. This checks Rust
types and Windows bindings, not native execution, full-workspace Windows linking
or runtime measurements. The interaction diagnostic now requests snapshots on
all three supported operating systems and checks platform-specific allocator
availability in its test. Native Windows and Linux execution remain outstanding;
this supersedes the previous unsupported-Windows implementation note only.

### Retained RSS deltas

Geometry diagnostics now report `geometryRssGrowthBytes` and
`materializedPlannedRowsRssGrowthBytes` from adjacent retained-memory snapshots,
matching the source's two RSS subtraction boundaries. Signed arithmetic preserves
decreases without unsigned underflow; unavailable samples produce `null`.
Tests cover both directions across the complete unsigned counter range, equal
samples and missing samples. All three native-memory tests and all three geometry
tests (including both frozen source oracles) passed, as did xtask Clippy and
formatting. These are current-process deltas, not peak-memory or JSC-heap parity.

### Indexed highlighter result retention

The preparation controller now indexes borrowed `(runtime_id, content_identity)`
pairs before retaining merged results, replacing a full file-list scan for each
cached result. It preserves the previous existential match, including duplicate
IDs with different content identities, without cloning diff trees. Retention is
now logarithmic per lookup rather than linear in the current file count.
All 57 highlighter-filtered TUI tests passed, including 600-file cache retention,
reload invalidation, deferred publication and actual stack/split cell painting.
TUI all-target Clippy and formatting passed. No latency improvement is claimed
without a measured run; the complete hook record remains unmapped.

### Workspace verification at `3c6c4701`

`CARGO_INCREMENTAL=0 cargo test --locked --workspace --all-targets` passed on
the native macOS host with the checkout unchanged throughout the run. This includes
all 93 terminal-pager tests, 25 native highlighter integration tests, 572 session
tests, 1,094 TUI tests, 241 VCS tests and 203 tooling tests with one existing test
ignored. Full-workspace all-target Clippy with `-D warnings` and formatting passed.
`cargo deny check` passed under the existing policy, including its three existing
ignored advisories and duplicate-dependency warnings; no policy was relaxed.
The observed suite summaries and command outcomes are retained in
`verification-3c6c4701.json`.

VCS watcher tests took 246.93 seconds but finished successfully. A diagnostic
process sample during the wait showed watcher close waiting for notify's FSEvents
thread, which was in a native current-event-ID RPC. The run was not interrupted
or restarted. This checkpoint does not establish cross-platform native execution,
benchmark acceptance, full source parity or release readiness, and is not a run
of the composed `cargo xtask verify` command.
The fresh strict port audit still fails with 290 unmapped records across the
1,257-file baseline and 11 cached upstream commits pending; coverage was unchanged.

### Terminal-theme probe tooling (partial)

`cargo xtask themes probe` ports the diagnostic entry point from
`scripts/probe-terminal-theme.ts`: it opens controlling-terminal input, sends
OSC 11 through stdout when attached or terminal output otherwise, uses a 500 ms
deadline, and writes JSON diagnostics to stderr. Fields preserve source names:
`mode`, `color`, `classified`, `raw`, `stdoutIsTTY`, and `stdinIsTTY`. Recorded
response chunks replace escape characters with the printable `\\e` notation.
The existing detector restores input raw mode and owned terminal handles close
when the command returns. Windows uses `CONIN$`/`CONOUT$` instead of `/dev/tty`.

The focused test covers fragmented white-background responses, exact report
fields/query bytes, no-response output and restoration of raw mode. xtask
all-target Clippy and formatting passed. Actual PTY/source-oracle exchanges and
cross-platform command lifecycle validation are still outstanding; the source
record remains unmapped.

The native Unix integration test `xtask/tests/theme_probe_pty.rs` now launches
the actual tooling executable on a fresh PTY, waits for the complete OSC 11 query,
and verifies a black-background response and a separate unanswered-query case.
It checks successful exit, exact response diagnostics and terminal attachment
flags. Both cases passed on macOS. This is native command evidence, not a frozen
Hunk comparison, Windows execution or a real-terminal raw-mode restoration check.

The subsequent PTY test also compares the complete terminal settings before
launch and after successful process exit. Exact restoration passed on macOS
for both the background-response and unanswered-query paths, superseding the
real-terminal restoration limitation above for those two paths only.

The PTY matrix now also redirects stdout to a temporary file for each response
case. All four cases passed: the OSC query still reaches the controlling PTY,
the redirected file remains empty, `stdoutIsTTY` reflects redirection, and the
complete terminal settings are restored. This verifies the non-terminal output
fallback on macOS; stdin is still attached in these cases. xtask Clippy and
formatting passed, and source-oracle and Windows coverage remain outstanding.

The matrix now covers all eight combinations of response/timeout and attached
or redirected stdin/stdout. Redirected stdin is a file descriptor shared with
the parent test shell; after the probe exits, the shell reads its remaining
bytes and verifies the complete sentinel is untouched. All eight macOS PTY
cases passed, including terminal flags and full termios restoration. This
supersedes the attached-stdin limitation above; an actual pipe and Windows
transport remain separate unverified cases. xtask Clippy and formatting passed.

Frozen timeout diagnostics from both source pins are now recorded in
`oracles/theme-probe-timeout.json` using the declared Bun 1.3.14 runtime and
blob-verified script/detector files. The native PTY test compares the full timeout
JSON against both captures and passed. Source processes remained alive after
printing diagnostics and a subsequent five-second poll; explicit Ctrl-C cleanup
then exited with code 1. Those are interrupted exits, not natural source exit
codes. This lifecycle difference remains unresolved and the record stays unmapped.
Earlier exploratory Bun 1.3.5 runs showed the same symptom but are not the retained
runtime evidence. All disposable probe processes were stopped; no runtime or
TypeScript mirror was added to the product.

The native probe matrix now has 12 cases: response/timeout, attached/redirected
stdout, and terminal/file/pipe stdin. The actual-pipe cases run the probe and
then drain the same pipe, proving the sentinel bytes remain unread. All cases
passed on macOS, along with the frozen timeout JSON comparison, exact terminal
restoration, xtask Clippy and formatting. This closes the previous native Unix
pipe-input test gap, not the source lifecycle or Windows parity gaps.

### OSC response scanning correction

Reviewing the pinned detector exposed a runtime parser mismatch: the Rust
implementation stopped at the first OSC prefix and preferred any later BEL
over an earlier string terminator. It also selected hex before a later RGB
response, unlike Hunk's RGB-first search. The parser now scans candidate prefixes,
uses the first terminator for each candidate, skips invalid payloads and preserves
RGB precedence. Five regression cases were executed against both blob-verified
pins under Bun 1.3.14; their outputs are frozen in
`oracles/osc-background-scan.json` and reproduced by the Rust regression test.
All five theme-detection tests, TUI all-target Clippy and formatting passed.
The separate diagnostic-process lifecycle discrepancy remains unresolved.

`osc_scanning_matches_both_frozen_source_captures` now loads the frozen JSON
directly and compares every captured case with the Rust parser. Both exact
source commits are recorded alongside the shared blob. All six detector tests
and TUI all-target Clippy passed; this makes the captured expectations executable
without requiring Bun during normal Rust tests.

The actual probe PTY matrix now includes an echoed OSC query immediately before
a valid RGB response across all stdin/stdout routes (18 total cases). It verifies
that the corrected shared parser reaches a dark-mode result rather than timing
out, while preserving the entire escaped response and restoring terminal settings.
The macOS matrix, xtask all-target Clippy and formatting passed. Source process
lifecycle and native Windows checks remain open.

The detector regression suite now exercises every two-chunk split of combined
hex/RGB responses and invalid-prefix/valid-response streams. A completed hex
response settles immediately and leaves later chunks unread; RGB precedence
applies only to data already received. Both initial raw-mode states are covered.
All seven detector tests, TUI all-target Clippy and formatting passed.

### Extension directory activity indexing (partial)

`cargo xtask extension-catalog activity-index` reads a GitHub topic-search JSON
payload from stdin and emits a repository-keyed activity object. The native
translation preserves absent metadata fields, ignores malformed entries, folds
repository-name case and keeps the last duplicate entry. It performs no network
requests and writes no repository state. Focused tests and xtask all-target
Clippy passed. The catalog data, fetch/fallback workflow, page rendering and
remaining directory helpers are not covered by this increment; the source
`website/src/data/extensions.ts` remains unmapped.

The executable integration test `xtask/tests/extension_catalog_cli.rs` verifies
JSON stdin/stdout behavior, absent fields versus zero stars, empty activity for
null payloads, malformed JSON, trailing data and rejected extra arguments.
Successful commands leave stderr empty; rejected input emits no partial stdout.
The CLI test, xtask all-target Clippy and formatting passed. This remains a
partial directory-data port, not website or catalog parity.

`oracles/extension-activity-index.json` freezes seven activity-index cases from
each pinned source version under Bun 1.3.14. Exact commits and the differing
source blob hashes are retained. No network fetch was invoked. The Rust test
loads every case directly, covering malformed payloads, omitted metadata,
duplicate repository keys and Unicode lowercasing; both focused tests passed,
as did xtask all-target Clippy and formatting. The remaining directory source
interval is still unmapped.

`cargo xtask extension-catalog json-ld` translates the directory's script-body
escaping helper: JSON is serialized and every literal `<` becomes `\u003c`, so
repository descriptions cannot close an enclosing script element. Decoded JSON
values remain unchanged. Unit and actual-command tests cover script closers,
markup in keys/nested values, Unicode and literal escape text. Both tests,
xtask all-target Clippy and formatting passed. Website integration and complete
JavaScript/Rust serialization parity remain separate open work.

`oracles/json-ld-serialization-gaps.json` records six direct source/native
comparisons at `7af511e3`. Both pinned source versions agree. Four cases differ:
integral floats, negative zero, the fixed-decimal/exponent boundary at `1e20`,
and numeric-property ordering. The `1e-7` and ordinary insertion-order cases
already agree. The fixture is explicitly an unresolved-gap record, not passing
parity evidence. The replacement needs ECMAScript number formatting and numeric
property enumeration without losing insertion order for ordinary keys.

Numeric-property enumeration is now implemented recursively for JSON-LD:
canonical array-index keys below `4294967295` sort numerically, followed by
ordinary keys in insertion order. The tooling explicitly enables serde's
insertion-order feature instead of relying on transitive feature activation.
Both frozen property-order cases and nested/boundary tests passed, along with
the escaping test, xtask Clippy and formatting. The three observed number-format
differences remain unresolved; historical gap captures are retained unchanged.

The JSON-LD serializer now uses native `ryu-js` formatting after conversion to
JavaScript's double-precision number representation. All six captured cases from
both pins pass, including integral floats, negative zero and `1e20`. Recursive
property ordering and less-than escaping remain in place. Focused tests, xtask
Clippy, formatting and the existing-policy dependency/license audit passed; the
new tooling dependency is recorded in Cargo.lock and THIRD_PARTY_NOTICES. The
historical gap fixture remains unchanged as provenance. This closes those four
observed cases, not all possible JSON parsing/serialization differences or the
remaining website port.

The executable-level JSON-LD test now feeds every frozen source case through
the CLI's actual JSON parser and serializer and compares exact stdout (plus the
command's trailing newline), successful exit and empty stderr. All three catalog
CLI integration tests and xtask all-target Clippy passed. Historical mismatch
observations stay in the fixture; the test uses the captured source expectations.

`oracles/json-ld-number-boundaries.json` adds six cases per pin for integer
rounding beyond the safe-integer range, signed/unsigned 64-bit endpoints,
fixed/exponential notation boundaries and the smallest positive subnormal.
The actual CLI matches all captured strings from both baselines. All three
catalog integration tests, xtask Clippy and formatting passed; this is broader
numeric evidence, not a waiver for untested JSON input semantics.

`cargo xtask extension-catalog format-updated` accepts JSON with `pushedAt`
and optional `now` RFC 3339 or date-only ISO timestamps and emits the coarse recency string or
null for unusable/future timestamps. The translated elapsed-time formatter
preserves source thresholds and pluralization, including `0 years ago` at
360–364 days. Boundary, future-time and subtraction-overflow tests passed,
as did xtask Clippy and formatting. JavaScript's broader Date parsing and the
full website integration remain unported; this is not a completed ledger mapping.

`oracles/extension-recency.json` records 12 actual results per pin under Bun
1.3.14, including month/year boundaries, offset-equivalent timestamps, future
timestamps and invalid input. Source undefined is explicitly encoded as null.
The CLI test feeds each captured input through the native command and compares
its result. All four catalog integration tests, xtask Clippy and formatting
passed. The broader JavaScript Date-input domain is still unverified.

`oracles/extension-recency-date-inputs.json` adds eight actual cases per pin:
date-only UTC values, short-month rollover, invalid days/months and leap-second
rejection. The native CLI compares both captured sets, including date-only
`now` values. Legacy Date spellings and offset-free date-time inputs remain
unimplemented; this additional coverage does not complete the source mapping.

`cargo xtask extension-catalog category-facets` accepts a JSON array of listings
with `categories` arrays and emits populated categories, ordered by descending
count then alphabetical name. It uses the eight-category source vocabulary and
counts duplicate occurrences exactly as the source does. Unknown categories
are rejected at the typed tooling boundary. `oracles/extension-category-facets.json`
captures five cases per pin, including every category tied and the actual
category arrays of each pinned catalog (which differ between pins). These are
metadata fixtures, not claims that the original TypeScript extensions can run
in Workdeck. The native CLI test compares each captured result and checks
invalid-input failures. Website integration and full catalog migration remain
incomplete.

### Extension directory activity loading (partial)

`cargo xtask extension-catalog load` reads listing objects with `repo` strings
from stdin and resolves their GitHub activity. It searches the branded
`workdeck-extension` topic once (eight-second deadline), then concurrently
fetches missing repositories (five-second deadlines). Matching is
case-insensitive for topic results; listing order and static fields survive.
Missing activity remains absent, and direct-request failures do not fail the
build. `GITHUB_TOKEN`, when present, is sent only in the authorization header;
transport error details are not printed. Tests exercise overlapping fallback
requests, deadlines, missing fields, ordering, empty/malformed topic results
and topic failure followed by successful direct lookup.

This is not yet wired into the Zola build or a migrated native catalog. HTTP
transport, exact source warning/exit behavior, and dual-pin loader oracle
comparisons remain unverified. The generic redacted topic-failure warning is
not an exact source-output parity claim. The source ledger remains incomplete.
The two loader unit tests and all six catalog CLI integration tests passed,
as did xtask all-target Clippy and the existing dependency audit policy.

`oracles/extension-loading.json` freezes three actual loader executions per pin
under Bun 1.3.14 with deterministic fetch responses: empty, partially populated
and failed topic search, with alternating successful and HTTP-503 direct
lookups. Results are projected to repository, name and activity fields; the
fixture also retains requested URLs and source warnings. Rust comparisons
verify all projected entries, the request multiset (concurrent ordering is
unspecified), and nonfailure warnings. The failed-topic diagnostic is retained
but not claimed equivalent to the native redacted warning. This does not test
the real HTTP transport or complete the source mapping.

The loader's HTTP transport now has a loopback-server test covering actual GET
paths, Accept/User-Agent headers, optional bearer authorization (synthetic test
token only), JSON objects/null/arrays, HTTP-503 rejection and malformed JSON.
The production request function is exercised directly, without a mocked HTTP
client. Server accept/read/write operations are bounded. This is local HTTP
evidence, not HTTPS certificate, redirect, timeout or live GitHub parity proof.

A separate silent-server test exercises the actual global request deadline:
the peer accepts but sends no response, and the client must return a typed
timeout before the server's safety ceiling. The socket is kept open until the
client returns, ruling out a premature EOF as the cause. This verifies timeout
enforcement locally; TLS, redirects and full upstream transport parity remain
unverified.

The HTTP loader explicitly accepts only 2xx responses, matching source
`response.ok`; non-2xx statuses remain typed through the topic-search warning.
Loopback cases include 302 without Location, 304, 404 and 503 in addition to
success and malformed JSON. Status-specific warning tests verify that direct
fallback still runs for 302/304/403/404/429/503. Redirect-following and generic
network-error diagnostic parity remain separate unverified behavior.

The status test exposed ureq's generic error for a 302 lacking Location. The
loader now handles redirects explicitly: only 301/302/303/307/308 with Location
are followed, at most 20 hops, under one shared deadline. Authorization is
removed on an origin change. Missing Location retains the original status;
non-HTTP destinations and credential-bearing redirect URLs are rejected.
Redirect chains and cross-origin credential stripping still require dedicated
executable parity tests; this implementation is not a completed mapping.

The redirect transport test now makes actual two-hop loopback requests for a
relative same-origin Location and an absolute cross-origin Location (different
port). It verifies the final JSON and GET paths, retention of a synthetic
authorization token on the same origin, and absence of authorization at the
other origin. Server accepts and socket I/O are bounded. Multi-hop limits,
TLS and comparison with the pinned runtime's redirect behavior remain pending.

`oracles/extension-redirect-authorization.json` now captures both pinned loaders
under Bun 1.3.14 with their initial GitHub request routed to local HTTP servers.
Native Bun fetch handles the redirects, preserving the source headers and
deadline. Both pins retain authorization for the same origin and remove it for
a different port. The native transport test compares its observed method,
path and authorization presence against both captures. The fixture records
only presence of a synthetic token, never credentials. This closes those two
HTTP redirect cases, not TLS, redirect limits or the whole source mapping.

### Legacy community directory migration page

`site/data/legacy-extensions.json` preserves all 16 baseline catalog entries,
their source fields, the baseline/blob anchors and MIT attribution. Every entry
is explicitly marked `requires-rust-rewrite`. The Zola `/extensions/` page
renders those records as migration references, with no installation command or
claim that a TypeScript extension is loadable. Names and summaries remain
historical source metadata. Native replacements are not inferred or invented.

`cargo xtask site check` now checks and builds the actual site in a temporary
directory, verifies that every card and rewrite warning renders, and rejects
installation commands or script tags in this legacy page. The unused generated
JavaScript search index is disabled; a JavaScript-free site search replacement
is still pending. Highlighting configuration uses Zola 0.22+'s
`markdown.highlighting` table. This is an initial migration page, not full
directory search/filter/sort, visual, accessibility or source-ledger parity.
Validated locally with Zola 0.22.1 macOS arm64 (release archive SHA-256
`46ac45a9e7628dba8593b124ee8794f4f9aa1c6b569918ecd4bbc5d0be190515`):
the composed site check/build, xtask all-target Clippy and formatting passed.

The site gate additionally validates pinned catalog anchors, exactly 16 unique
repository identities, nonempty names/summaries/categories, recorded versions
and source API numbers, and mandatory `requires-rust-rewrite` status. Every
repository must appear exactly once as a rendered link. Mutation tests reject
missing entries, duplicates, malformed repository strings, blank metadata,
unknown categories and false native compatibility. These checks protect the
migrated seed; they do not prove source runtime or interactive site parity.

The site check now reads the catalog's source directly from the pinned Git
commit and compares every original field and list position with migrated JSON.
A narrow Rust literal decoder accepts only JSON strings, integer literals,
punctuation, trailing commas and the six declared field identifiers; it never
executes TypeScript and rejects other syntax. The only added field excluded
from comparison is the separately validated rewrite status. A structurally
valid summary change fails this source comparison. Source refs must therefore
be available when checking the site. No TypeScript mirror or runtime is added.

The migration directory now offers a single selected capability filter using
native radio controls and CSS `:has`, without application JavaScript. Facet
counts/order are generated by the Rust helper and verified against all entries
during site checking. Unsupported CSS engines retain the full list rather than
losing content. Browser checks observed All=16, Command=12, Pane=9,
Changeset transform=4, Line highlighter=4, Keyboard mode=2, Theme=2 and
VCS backend=2. ArrowRight moved from Theme to VCS backend, and a 320-pixel
viewport had no horizontal overflow. The development server reported only a
missing favicon; generated production pages contain no script tags. This is
not Hunk's complete search/sort/pagination or button-interaction parity, nor a
full accessibility/visual audit. No source-ledger completion is claimed.

Facet regression checks explicitly reject a changed count, reordered facets
and an omitted facet while preserving the underlying listings. All 14 Rust
catalog-module tests pass together, including source comparisons and loopback
transport tests; xtask Clippy and formatting pass. This is a subsystem
checkpoint, not the workspace-wide or release completion gate.

Literal-decoder regression cases retain escaped quotes, backslashes, Unicode
and syntax-looking punctuation inside summaries while accepting trailing
commas outside strings. Calls, undefined values, negative/fractional literals
and comments are rejected by this deliberately restricted pinned-source
decoder. Supporting arbitrary TypeScript is neither required nor claimed.

`cargo xtask extension-catalog seed` regenerates the complete migrated catalog
JSON from the pinned Git source, including provenance, mandatory rewrite
status and Rust-computed facets. It reads no stdin, performs no network access
and writes only stdout. The CLI test compares every resulting JSON value with
the tracked catalog and rejects extra arguments. Regeneration does not complete
the surrounding source-ledger disposition or replace missing site functionality.

The tracked catalog is now canonicalized to the Rust seed output. The CLI
regression compares the complete stdout byte sequence, including formatting,
key ordering and final newline, against `site/data/legacy-extensions.json` in
addition to its semantic comparison. This makes this data artifact exactly
reproducible from pinned Git; the surrounding source file remains incomplete.

### Native release-fragment authoring (partial)

`cargo xtask changelog add <id> <patch|minor|major|empty> [body]` creates a
Workdeck fragment under `release/fragments/`, without invoking the source package
runtime. Invalid requests create no state; atomic no-clobber publication
preserves existing fragments. Tests cover all bump types, Unicode content,
maintenance-only fragments, overwrite rejection and invalid arguments.
See [native release fragments](../../docs/native-release-fragments.md).
Version preparation, consumption, prerelease policy and publication remain
unfinished, so `.changeset/README.md` remains unmapped.

`cargo xtask changelog status` adds read-only pending-fragment parsing and JSON
output with deterministic ID ordering and the highest requested bump. An
absent fragment directory remains absent. Malformed frontmatter, unsupported
products/bumps, empty user-facing notes and nonempty maintenance-only notes
are rejected. Four fragment tests, xtask Clippy and formatting passed; release
preparation and the complete source workflow remain unfinished.

`cargo xtask changelog plan` reads locked/offline Cargo metadata and pending
fragments to report a stable-version proposal with `applied: false`. Tests
cover bump precedence, component resets, maintenance-only/no-change plans,
overflow and explicit rejection of prerelease/build metadata. The actual
checkout returned `0.1.0` to `0.1.0` with no fragments. Seven fragment tests,
Clippy and formatting passed. Applying plans and prerelease integration remain
unfinished; this is not release readiness.

The probe's subsequent failure-path test injects resume, input-read, output-write
and output-flush failures with both initial raw-mode states. All eight cases
preserve the originating I/O error and restore the exact prior raw-mode state.
Both probe tests, xtask all-target Clippy and formatting passed. This validates
the diagnostic wrapper's delegation but does not replace real PTY lifecycle tests.
