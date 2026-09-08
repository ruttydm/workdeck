# Hunk semantic-port ledger

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
