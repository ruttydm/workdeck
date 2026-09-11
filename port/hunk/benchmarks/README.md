# Review-stream performance investigations

These reports expose an outstanding release-gate failure. They are **not** parity certification.

## Native follow-up receipts

The post-baseline highlight-loading change (`f86a04ed`) is represented by the
Ratatui/syntect path in `crates/workdeck-diff/src/syntax.rs`. Syntax grammars are
compiled once and embedded as raw bytes with `include_bytes!`; the executable never
base64-decodes a large text payload. The `bundled_highlighting_uses_raw_embedded_asset_bytes`
test exercises the same first-use token output while proving the generated asset is
present.

The fixture-overhead change (`6ba8465a`) is represented by the native benchmark fixture
generator in `xtask/src/benchmark/fixtures.rs`. Disposable Git repositories write their
author/signing configuration in one file operation before committing, preserving the
source fixture identity without repeated configuration subprocesses.

On 2026-09-07 UTC (2026-09-08 local), three subprocess samples per implementation ran sequentially
on the same Apple M1 Pro, macOS 26.5.2 arm64 host after compilation and test jobs had finished.
Workdeck used Rust 1.95.0 (`59807616e`, 2026-04-14) and the
default optimized Cargo release profile at `28a8854a49e4acabc9c27a03a02dc69f3857680c`.
Both disposable Hunk checkouts used Bun 1.3.14 with update notices and MCP disabled.
Other host activity was not controlled; no peak-memory comparison was captured.

| Median, milliseconds | Hunk main 2c00f435 | Hunk v0.20.1 | Workdeck 28a8854a | Workdeck 9780b4cf | Workdeck f62e190e |
| --- | ---: | ---: | ---: | ---: | ---: |
| Cold first frame | 2.40 | 2.08 | 237.62 | 170.51 | 158.30 |
| Warm first frame | 1.76 | 1.56 | 235.13 | 168.76 | 156.36 |
| Four wheel ticks | 197.42 | 207.60 | 1815.55 | 1389.65 | 1293.14 |

All three native timings exceed the 10% limit against both pins. The report schema's inherited
15%-plus-absolute thresholds are historical compatibility metadata, not Workdeck's acceptance gate.
All reports agree on 180 files, 120 lines per file and four scroll ticks.

The second native run used the same optimized profile and three subprocess samples, after all
verification jobs finished. Commit `9780b4cf` removes full semantic hunk/source projections from
TUI filtering by borrowing the three queried fields. Cold/warm medians improved about 28%, and
wheel-tick latency about 23%, relative to the first native run. Both native versions still fail
the latency gate by a wide margin. No memory or full-process launch acceptance is implied.
The same change corrects query whitespace against a dual-pin oracle (U+0085 is not trimmed;
U+FEFF is), with a reproduced failing test before the correction.

The third native run (`f62e190e`) uses borrowed metadata serialization for highlight fingerprints.
Byte-for-byte JSON and digest tests preserve the existing cache identity, including Unicode and
full source snapshots. The same optimized three-sample procedure improves these medians another
7–8% versus `9780b4cf`, but still fails the latency gate. This report predates shared token-cache
reads and cannot establish their performance impact.

The shared-token-cache run (`dc3041ab`, including implementation `abcd918b`) records cold/warm
medians of 163.35/156.89 ms and four-wheel-tick median 1273.56 ms, again with three optimized
subprocess samples on this host. Compared with the preceding run, scrolling is about 1.5% faster
but cold/warm rendering is slightly slower. These small mixed differences do not establish an
overall performance improvement; the latency gate still fails and memory remains unmeasured.

The source workload excludes fixture/app construction and highlight-settlement cleanup from its
timers. Therefore first-frame results are not full-process launch results. A separate debug CPU
sample caught `ReviewApp::new` → cursor seeding → review-row construction → highlighting;
that observation does not quantify optimized startup cost or establish a complete profile.

Reproduce the native report from its recorded commit:

```console
cargo build --release -p xtask
target/release/xtask benchmark run --script large-stream.ts --samples 3 --out REPORT.json
```

For each pinned disposable oracle checkout, execute the original benchmark runner with Bun 1.3.14:

```console
HUNK_MCP_DISABLE=1 HUNK_DISABLE_UPDATE_NOTICE=1 bun benchmarks/run.ts --script large-stream.ts --samples 3 --out REPORT.json
```

The Bun command is oracle-only, never a Workdeck runtime/tooling dependency or installation path.
Raw reports retain every sample, source Git SHA, runtime identity, quantile and structural metric:

- [Pinned main](large-stream-hunk-2c00f435.json)
- [Pinned stable](large-stream-hunk-v0.20.1.json)
- [Optimized native before performance fixes](large-stream-native-28a8854a.json)
- [Optimized native after borrowed-field filtering](large-stream-native-9780b4cf.json)
- [Optimized native after borrowed fingerprint serialization](large-stream-native-f62e190e.json)
- [Optimized native with shared token-cache reads](large-stream-native-dc3041ab.json)

## Non-ASCII stream

The native Unicode workload at `dfc2f4e8` ran three optimized subprocess samples, followed
sequentially by three samples from each pinned Hunk checkout on the same host and runtimes
described above. Compilation and native verification had finished before these runs. Other host
activity was not controlled. Hunk emitted its React `act(...)` environment warning; the reports
retain the workload's measured results, not a claim that its scheduler matches Ratatui's.

| Median, milliseconds | Hunk main | Hunk stable | Workdeck |
| --- | ---: | ---: | ---: |
| Cold first frame | 19.48 | 19.00 | 146.11 |
| Per-run median wheel tick | 1.79 | 2.19 | 259.83 |
| Per-run p95 wheel tick | 11.48 | 7.62 | 276.14 |

The latter rows aggregate each subprocess's eight-tick median/p95, not all ticks pooled together.
Every report has 120 files, 120 lines per file and eight ticks. These measurements fail the 10%
latency gate against both pins. No peak-memory or full-process launch acceptance is established.
Unlike large-stream, this source workload has no explicit 17 ms pause between wheel dispatch and
render; its native counterpart measures dispatch, render and a thread yield per tick.

Use the commands above with `--script non-ascii-stream.ts` to reproduce these reports:

- [Pinned main Unicode stream](non-ascii-stream-hunk-2c00f435.json)
- [Pinned stable Unicode stream](non-ascii-stream-hunk-v0.20.1.json)
- [Native Unicode stream](non-ascii-stream-native.json)

## Wrapped Japanese Markdown

At native commit `8ee94d28`, three optimized subprocess samples ran before three samples from
each pinned Hunk checkout, sequentially on the same host after builds and tests completed.
Runtime versions and uncontrolled-other-host-activity limitations are as above. Stable Hunk
emitted React `act(...)` environment warnings. Both pins have identical workload source bytes
(SHA-256 `5a081ec10b112099d2e17405c9e93d692e00ca72567ff2d5b264152f7f83c8a3`).

| Median, milliseconds | Hunk main | Hunk stable | Workdeck |
| --- | ---: | ---: | ---: |
| 518-line mount to first frame | 103.82 | 68.98 | 146.55 |
| Long-line mount to first frame | 20.49 | 21.69 | 3.89 |
| Immediate twelve-event wheel burst | 4.72 | 4.25 | 170.86 |
| Settled wheel burst | 29.47 | 28.00 | 208.45 |

All nine samples preserve 518 physical lines, 8,736 UTF-16 units in the long line, twelve burst
events and 54/55/55 initial/immediate/settled Japanese-content rows. First-paint timers include
app construction but exclude fixture construction. Burst timers exclude synchronous syntax
preparation and initial viewport settlement. Native highlighting is explicitly prepared in its
per-app cache, rather than a source module-global cache. Movement checks compare characters,
not syntax styles. First-frame content must occupy at least 80% of the 60-row viewport; burst
content must retain at least 80% of the initial content rows.

The long-line result alone beats both pins; the other three timings fail the 10% latency gate.
No peak-memory or full-process launch acceptance is established. Reproduce using the preceding
commands with `--script wrapped-cjk.ts`:

- [Pinned main wrapped CJK](wrapped-cjk-hunk-2c00f435.json)
- [Pinned stable wrapped CJK](wrapped-cjk-hunk-v0.20.1.json)
- [Native wrapped CJK](wrapped-cjk-native.json)

The follow-up native run at `bf92f7bb` avoids syntax preparation while rebuilding wheel-limit
geometry. Three optimized samples after build completion retain the same 54/55/55 content rows.
Its medians are 148.11 ms for 518-line first paint, 3.94 ms for long-line first paint, 152.95 ms
for the immediate burst and 193.39 ms for the settled burst. Immediate/settled burst medians are
about 10.5%/7.2% lower than `8ee94d28`; first-paint medians are slightly higher. This does not
satisfy the 10% regression limit versus either Hunk pin, nor establish a memory improvement.
The row planner still runs per event; no potentially stale geometry cache was introduced.

- [Native wrapped CJK with geometry-only wheel limits](wrapped-cjk-native-bf92f7bb.json)

## Control-free width borrowing follow-up

Commit `8395004e` avoids allocating sanitized copies for control-free width input. After its
optimized build completed, three subprocess samples each ran sequentially for wrapped-CJK and
non-ASCII stream on the same host. The combined report retains both workloads and all samples.

Wrapped-CJK medians are 146.75 ms (518-line first paint), 3.91 ms (long-line first paint),
151.49 ms (immediate burst), and 192.42 ms (settled burst). These are within about 1% of the
preceding run: they do not establish a meaningful end-to-end latency gain from this allocation
change. Content rows remain 54/55/55.

Non-ASCII medians are 147.26 ms (cold frame), 217.74 ms (per-run median tick), and 220.56 ms
(per-run p95 tick), with unchanged 120-file/120-line/eight-tick counts. The last comparable native
report was `dfc2f4e8`, before the geometry-only wheel-limit change; the roughly 16% reduction in
median tick latency cannot be attributed solely to width borrowing. Both workload latency gates still
fail against the pinned Hunk reports. Memory remains unmeasured.

- [Combined native Unicode follow-up](unicode-native-8395004e.json)

### Interaction-latency translation in progress

`xtask/src/benchmark/interaction_latency.rs` exercises the source's 180-file, 120-line,
240x28 workload with six real `]` presses and eight wheel ticks on separate renderers. It checks
selection changes, viewport movement and finite timing samples. The diagnostic command
`cargo xtask benchmark interaction-diagnostic` now reports raw timing arrays plus native RSS and
malloc-zone snapshots after first frame and navigation on macOS. It is explicitly not the source
benchmark command: cross-runtime heap semantics and same-host optimized comparisons remain missing.
Frozen single-run output from both pinned anchors is now recorded in
[the interaction oracle](../oracles/benchmark-interaction-latency.json), covering all 13 metric
names and source-scale counts. Those sequential original-runtime captures are diagnostic evidence,
not repeated performance acceptance. The source ledger record is covered by the native translation;
the default runner measures Workdeck only, so this receipt is not a cross-runtime parity result.

`cargo xtask benchmark memory-snapshot` provides a macOS native diagnostic: current process
`rssBytes` from `proc_pidinfo(PROC_PIDTASKINFO)` and `mallocInUseBytes` summed across malloc zones.
The live native test reads both counters and repeats the read with a retained allocation. These
are current snapshots, not peak usage; malloc zone usage is not JavaScript `heapUsed`, and no
GC-equivalent operation is claimed. Linux/Windows backends currently return an explicit unsupported
error. Counters are integrated at the interaction workload boundaries, outside measured input/frame
intervals. Defining/verifying cross-runtime memory comparisons remains required before the source
interaction benchmark can be mapped or admitted. The diagnostic does not force allocator purges
and cannot claim equivalence to Hunk's full-GC snapshots.

### Current native interaction receipt at `0e85dcab`

The plain-split repaint cache, pending worker-key memo, shared review metadata, and the
source-presentation-keyed geometry cache are measured in
[`interaction-native-ee34bf1e.json`](interaction-native-ee34bf1e.json). Twenty optimized Workdeck
samples ran on the macOS arm64 host after the full verifier. The report is a native receipt;
the paired comparison below uses the frozen disposable-oracle Hunk pins from the same host.

| Median | Workdeck |
| --- | ---: |
| First frame | 4.67 ms |
| Navigation press | 3.34 ms |
| Scroll tick | 0.60 ms |
| Scroll tick p95 | 0.62 ms |
| RSS after first frame | 112,525,312 bytes |
| RSS after navigation | 115,933,184 bytes |

Reproduce it with:

```console
cargo build --locked --release -p xtask
target/release/xtask benchmark run --script interaction-latency.ts --samples 20 --out REPORT.json
```

The geometry cache was previously disabled whenever any file carried a snapshot-backed source
(`SourceOrigin::File`), which describes every interaction fixture file, so each wheel tick
rebuilt the complete row plan inside dispatch. Availability now keys the geometry and
plain-height caches through a monotonic presentation revision, and the presentation table is
Arc-shared, so per-frame option clones no longer deep-copy it. The immutable-document pair cache
remains bounded to 2,048 entries and invalidated by document identity, selection, theme, width,
wrapping, line-number, highlight, and horizontal offset changes. Pending native worker requests
retain an identity-scoped key so redraws poll without rebuilding serialized metadata; completion
still follows the ordinary decode path. Dynamic comment, extension, and line-highlight paths
continue to use the complete metadata painter. A regression test pins cached and rebuilt frames
cell-identical across wheel ticks with snapshot-backed sources. Non-macOS receipts remain
required release evidence.

For a fresh paired diagnostic, the disposable oracle captures are
[`interaction-hunk-2c00f435-20.json`](interaction-hunk-2c00f435-20.json) and
[`interaction-hunk-v0.20.1-20.json`](interaction-hunk-v0.20.1-20.json). They used the available
Bun 1.3.5 (the historical receipt used Bun 1.3.14), so they are evidence for this host and not a
replacement for the pinned-runtime release run. Their timing medians compared with the native
receipt are:

| Median | Hunk main | Hunk v0.20.1 | Workdeck |
| --- | ---: | ---: | ---: |
| First frame | 20.70 ms | 21.28 ms | 4.67 ms |
| Navigation press | 51.95 ms | 53.22 ms | 3.34 ms |
| Scroll tick | 1.93 ms | 2.04 ms | 0.60 ms |
| Scroll tick p95 | 8.93 ms | 6.83 ms | 0.62 ms |

Workdeck is now faster than both pins on every paired interaction metric: scroll is 68.9% under
the main-pin median and 70.6% under the stable-pin median, so the strict 10% interaction latency
gate passes on this workload. Three fresh native rounds under `/usr/bin/time -l` peak at
119,013,376 bytes (116,785,152; 118,964,224; 119,013,376), below the frozen pin peaks of
613,482,496 and 591,872,000 bytes in [`interaction-paired-peak-c453574b.json`](interaction-paired-peak-c453574b.json)
and below the prior native paired peak of 170,754,048 bytes, so the no-peak-memory-regression
requirement also passes on this host. Hunk RSS includes a JavaScript heap and native process
footprint, whereas Workdeck reports native RSS and allocator snapshots; those memory values are
retained for diagnostics, not treated as directly comparable peak-memory proof beyond the
`/usr/bin/time -l` peak comparison above.

### Optimized interaction diagnostic at `10bb95d7`

[Raw nine-process comparison](interaction-diagnostic-10bb95d7.json) records three optimized native
runs, then three pinned main and three pinned stable runs on the same macOS arm64 host. Builds
and tests had finished first; unrelated host activity was not controlled. Using the source's
nearest-rank percentile rule, medians across runs are:

| Measurement | Native | Hunk main | Hunk stable |
| --- | ---: | ---: | ---: |
| First frame (ms) | 160.96 | 19.04 | 19.03 |
| Per-run median navigation press (ms) | 320.58 | 45.47 | 47.73 |
| Per-run median scroll tick (ms) | 228.02 | 1.49 | 1.32 |
| RSS after first frame (bytes) | 213811200 | 305725440 | 292356096 |
| RSS after navigation (bytes) | 238862336 | 444219392 | 435060736 |

All three latency measurements fail the user's 10% gate by a substantial margin. Lower RSS
snapshots do not establish peak-memory acceptance. Native malloc usage remains distinctly labeled
in the raw report and is not compared to JavaScript heap usage. Source parity and ledger mapping
remain incomplete. The next performance work must investigate per-event row planning/rendering;
changing the benchmark scale or excluding input dispatch would not resolve the observed gap.

### Geometry-only reveal follow-up at `4938978a`

[Three optimized native runs](interaction-diagnostic-4938978a.json) repeat the same workload after
removing syntax styling from reveal-only geometry calculations. Median navigation press latency
fell from 320.58 to 263.72 ms (17.7%). First frame is 158.25 ms and scrolling 227.71 ms: these
remain close to the preceding run and do not show a material improvement. Post-navigation RSS
median is 225443840 bytes, still a snapshot rather than peak usage. All latency gates remain
failed against both pinned Hunk reports; source ledger mapping remains incomplete.

### Cursor-geometry follow-up at `7bc6ed7e`

[Three further optimized runs](interaction-diagnostic-7bc6ed7e.json) report first-frame median
158.15 ms, navigation median 264.18 ms and scroll median 228.57 ms. These are effectively unchanged
from the prior geometry-only reveal result; no material latency gain is demonstrated for this
workload. The newly adjusted keyboard/cursor paths need separately targeted performance coverage.
All original interaction latency gates remain failed. Further optimization requires profiling the
remaining production rendering work, not changing workload counts or claiming success from tests.

[Native sampled profile](interaction-profile-7bc6ed7e.md) identifies SHA-256 compression,
allocation, syntax parsing and word differences among the major top-of-stack observations during
the optimized workload. It records the code correlation to repeated highlight identity construction
and the limits of the partial-process sample. Sampled timings are not added to benchmark reports.

The pinned `sha2` 0.10.9 source selects its ARM SHA-2 implementation only with the `asm` feature.
`workdeck-core` now enables that feature for aarch64 targets, retaining the dependency's runtime
CPU detection and software fallback. This changes neither cache-key inputs nor digest format.
Canonical empty, short, multiblock and million-byte SHA-256 vectors cover digest compatibility.
The feature adds the MIT-licensed `sha2-asm` dependency. No hash identity has been truncated or waived.

[Three optimized ARM-backend runs](interaction-diagnostic-18fdc4b9.json) now measure 126.79 ms
first frame, 236.75 ms median navigation and 201.78 ms median scrolling. Relative to `7bc6ed7e`,
these improve approximately 19.8%, 10.4% and 11.7% respectively with unchanged workload counts.
All still fail the 10% gate versus pinned Hunk. Post-navigation RSS median is 224755712 bytes;
peak usage and cross-runtime heap acceptance remain unproven. Native platform-matrix execution
also remains outstanding.

### Common-prefix word-diff follow-up at `7b8a0b48`

[Three optimized runs](interaction-diagnostic-7b8a0b48.json) report 115.57 ms first frame,
214.49 ms median navigation and 178.11 ms median scrolling. Compared with `18fdc4b9`, these
improve approximately 8.9%, 9.4% and 11.7%. The comparison table now excludes only forced equal
leading tokens, with full change sequences and emphasis ranges checked against the prior
algorithm for 67,081 short token pairs. Workload counts, viewport and input dispatch are unchanged.
All latency gates still fail against Hunk; current RSS median after navigation is 225214464 bytes,
not peak-memory acceptance. No source ledger record was newly mapped.

### Length-only emphasis follow-up at `c27fbc05`

[Three optimized runs](interaction-diagnostic-c27fbc05.json) report medians of 114.02 ms first
frame, 211.88 ms navigation and 175.09 ms scrolling. The roughly 1–2% change from the prior run
is small; no material end-to-end improvement is established. The allocation removal preserves
exact emphasis ranges, but does not address the dominant remaining cost. All pinned-source latency
gates still fail, peak memory remains unproven and source-ledger coverage is unchanged.

### Styled-run emphasis follow-up at `da5f06e0`

[Three optimized runs](interaction-diagnostic-da5f06e0.json) report 110.73 ms first frame,
206.47 ms median navigation and 169.75 ms median scrolling. This is a small (roughly 3%)
shift from `c27fbc05`, not a resolution of the remaining performance gap. The implementation
copies contiguous equal-style text runs while preserving character-start byte-range semantics;
Unicode splits and overlapping ranges are compared against the original character loop.
All pinned-Hunk latency gates still fail. Post-navigation RSS median is 226263040 bytes,
which is a current snapshot, not peak-memory evidence. Ledger coverage remains unchanged.

### Rejected word-emphasis memoization at `48ddc5fc`

[Three optimized runs](interaction-diagnostic-48ddc5fc.json) report 113.30 ms first frame,
209.71 ms median navigation and 173.36 ms median scrolling. These do not demonstrate a
benefit versus `da5f06e0`; malloc-in-use snapshots also increased. The bounded direct-mapped
cache passed correctness tests but is removed from production on this evidence. Its code
and tests remain recoverable in Git. The workload has 8,640 changed pairs competing for
4,096 slots, so cyclic eviction is a plausible contributor, not a measured attribution.
No cache-size tuning is accepted as parity evidence. The remaining investigation is the
cost of rebuilding offscreen rows while retaining exact geometry and all dynamic behavior.
All source latency gates remain failed; ledger coverage and peak-memory status are unchanged.

### Explicit unwrapped geometry at `3310d9d8`

[Three optimized runs](interaction-diagnostic-3310d9d8.json) report 110.92 ms first frame,
150.79 ms median navigation and 115.05 ms median scrolling. Against the last retained
implementation (`da5f06e0`), navigation improves about 27.0% and scrolling about 32.2%; first
frame is effectively unchanged. Geometry requests now skip painting unwrapped code, whose
row count is exact, while retaining note/gap/extension geometry and cursor targets. Wrapped
code still uses the existing builder; paint and copy always request complete text. Regression
coverage compares row maps and separately preserves painted-content comparisons across both
layouts, zero/narrow/normal widths, Unicode, unequal pairs, horizontal offsets and saved notes.
All 1,022 TUI unit tests and scroll integration pass. This reduces repeated input-path work,
but every pinned-Hunk latency gate still fails. Current post-navigation RSS median is
224755712 bytes; peak-memory acceptance remains unproven. No ledger mapping was added.

### Plain split viewport painting at `d2de3d88`

[Three optimized runs](interaction-diagnostic-d2de3d88.json) report 57.76 ms first frame,
94.18 ms median navigation and 58.44 ms median scrolling: approximately 47.9%, 37.5% and
49.2% lower than `3310d9d8`. Exact geometry is measured before clamping the scroll window;
offscreen plain split code then skips painting. Wrapped, annotated, expanded-source and
extension-customized reviews retain the complete painter. Copy and public rendering also
remain complete. The new differential test compares visible text/styles and all row maps
against the full painter; all 1,023 TUI unit tests and scroll integration pass.
These measurements do not pass any pinned-Hunk latency gate. Current post-navigation RSS
median is 214597632 bytes, not peak-memory acceptance. Source-ledger coverage is unchanged.

### Count-only gap planning at `a5fc87e9`

The [updated profile](interaction-profile-d2de3d88.md) motivated removal of source-line
allocations from collapsed-gap planning. [Three optimized runs](interaction-diagnostic-a5fc87e9.json)
report 53.75 ms first frame, 87.27 ms median navigation and 52.66 ms median scrolling,
roughly 6.9%, 7.3% and 9.9% lower than `d2de3d88`. The source-backed expansion API remains
available; count-only geometry shares its checked address arithmetic. Exhaustive normalization
counts and direct gap-address comparisons supplement the full review/TUI suites.
All pinned-Hunk latency gates still fail. Current post-navigation RSS median is 211288064
bytes, not peak-memory evidence. No ledger record was newly mapped.

### Highlight prefetch policy port

Inspection of pinned `DiffPane.tsx` confirms that Hunk selects highlight requests using
the selected file, adjacent files and a viewport halo of at least 24 rows or three viewport
heights, enlarged by rapid-scroll overscan. `highlight_prefetch.rs` translates the selection policy,
verified against [executed baseline/stable fixtures](../oracles/highlight-prefetch-policy.json).
The fixtures execute the unchanged extracted functions with each pin's actual file-section
intersection implementation, not a reimplementation used as its own oracle.
Plain unwrapped split reviews now wire that policy into live highlight requests. The native
clock state preserves first-read baselining, peak retention, positive-burst deadline extension,
160 ms idle expiry and the wrapped-scroll activation threshold. A live 40-file test verifies
that distant files are not requested until the viewport/halo reaches them, including an EOF jump.
Cursor initialization now requests geometry instead of eagerly highlighting the entire stream.
Complex/wrapped reviews still use the full painter and all-file highlighting; wrapped-window
warmup and full render-window lifecycle remain incomplete. This is not a complete `DiffPane.tsx`
port, and its ledger record stays unmapped. End-to-end performance must be measured separately.

### Live highlight halo at `e965fd7e`

[Three optimized runs](interaction-diagnostic-e965fd7e.json) report 20.10 ms first frame,
58.28 ms median navigation and 21.49 ms median scrolling. Against `a5fc87e9`, these decrease
about 62.6%, 33.2% and 59.2%. Benchmark setup, workload counts and input dispatch are unchanged;
production cursor initialization and live prefetch now avoid eager all-file highlighting.
Current post-navigation RSS median is 190545920 bytes, not peak-memory proof. The first-frame
result is close to earlier three-run source medians, but navigation and scrolling remain above
the source budget. Fresh paired-source comparisons and full benchmark/memory gates remain
required. No ledger coverage or overall performance gate is claimed complete.

### Refreshed alternating source comparison

[Three alternating native/main/stable rounds](interaction-paired-e965fd7e.json) retain raw
outputs and verify both source checkout anchors. Native first-frame median is 20.32 ms,
versus main 19.91 ms and stable 19.52 ms: within 10% in this sample, with substantial source
startup variability retained rather than discarded. Navigation medians are native 58.58 ms,
main 46.49 ms and stable 47.65 ms. Scroll medians are native 21.57 ms, main 1.29 ms and stable
1.21 ms. Navigation and scrolling fail the gate. These are current-RSS snapshots, not peak
memory comparisons, and the overall benchmark gate remains failed. The next performance
investigation should target native per-input geometry work, not claim completion from the
first-frame aggregate alone.

### Configuration-clone reduction at `4cca95b4`

[Three optimized runs](interaction-diagnostic-4cca95b4.json) report 20.10 ms first frame,
57.95 ms median navigation and 21.12 ms median scrolling. These are close to the preceding
measurements; removing repeated configuration clones does not demonstrate a material latency
gain. Mixed-width file tests verify that derived digit settings never leak between files.
The per-input performance gap remains, as do peak-memory and complete source-ledger gates.

### Selected-file extension projection at `53852d0e`

The [updated sample](interaction-profile-4cca95b4.md) exposed full-file serialization used
only to resolve one selected extension file. [Three optimized runs](interaction-diagnostic-53852d0e.json)
after narrowing that projection report 19.81 ms first frame, 42.70 ms median navigation and
21.12 ms median scrolling. Navigation improves about 26.3% over `4cca95b4` and is below the
last paired source medians; scrolling is effectively unchanged and still fails by a wide margin.
Duplicate/empty/missing IDs and line/hunk selection are compared against the original full
projection. Current post-navigation RSS median is 169771008 bytes, not peak-memory evidence.
Fresh paired-source results and the full benchmark gate remain outstanding; ledger coverage
is unchanged.

### Dispatch/render attribution at `230e582c`

[Three instrumented optimized runs](interaction-stages-230e582c.json) separate input handling
from the full render pass without changing the workload. Scrolling measures 3.92 ms dispatch
and 17.14 ms rendering, with 21.01 ms total median. Navigation measures 23.91 ms dispatch and
18.67 ms rendering, with 42.60 ms total median. These are independently aggregated nearest-rank
medians; sums of aggregate medians are not asserted equal. Two additional clock reads per
interaction instrument the boundaries, and per-sample tests ensure stage sums fit inside totals.
Rendering, not just the wheel handler's geometry rebuild, dominates the remaining scroll cost.
The next investigation should focus on the complete render pass. No performance or ledger gate
is passed by this diagnostic attribution.

### Unused extension-pane projection removal at `73f0d1d2`

[Three optimized stage-instrumented runs](interaction-stages-73f0d1d2.json) report 11.03 ms
first frame, 35.11 ms median navigation and 13.06 ms median scrolling. Scroll rendering falls
from 17.14 ms to 9.22 ms while dispatch remains about 3.88 ms. Frames without open registered
extension panes no longer serialize the complete visible-file metadata; open registered panes
retain the same availability/render payloads. The selected ID is read directly without another
file projection. All 1,029 TUI tests and scroll integration pass. Scrolling still fails the
pinned-source budget, and peak-memory, full benchmark and source-ledger gates remain incomplete.

### Exact wheel-height reuse at `465f55fc`

[Three optimized runs](interaction-stages-465f55fc.json) report 10.70 ms first frame,
35.26 ms navigation and 10.66 ms scrolling. Wheel dispatch falls from 3.88 ms to 1.39 ms;
rendering remains about 9.27 ms. A single retained immutable-document height avoids rebuilding
plain geometry just to clamp the wheel, with explicit setting/document invalidation and
fallback for complex content. Full-frame geometry and painting are not cached. The change
does not satisfy the pinned-Hunk scrolling budget; peak-memory and ledger gates remain open.

### Style-free section geometry reuse at `af2ff101`

[Three optimized runs](interaction-stages-af2ff101.json) report 10.65 ms first frame,
33.11 ms navigation and 8.36 ms scrolling. Navigation render time is 8.24 ms and scroll
render time 6.93 ms; dispatch remains 24.80 ms and 1.42 ms respectively. Eligible plain
renders reuse exact section positions instead of repeating the geometry prepass. The
1,031 TUI unit tests and scroll integration pass, including full-cell cached/rebuilt
comparisons after theme and terminal-width changes. This remains a diagnostic result,
not scrolling parity (the earlier paired Hunk sample was about 1.2 ms), peak-memory
acceptance, or additional ledger coverage. Other host activity was not controlled.

### Immutable menu input at `d5add8d1`

[Three optimized runs](interaction-stages-d5add8d1.json) report 8.09 ms first frame,
26.79 ms navigation and 4.31 ms scrolling. Navigation dispatch/render medians are
21.23/5.55 ms, and scroll dispatch/render medians are 0.024/4.28 ms. Menu projection
now borrows an immutable changeset snapshot instead of cloning every file body; its
matching, draft, filtering and bulk-action behavior is unchanged. The full 1,031-test
TUI suite and scroll integration pass. These diagnostic improvements still do not meet
the earlier pinned-source scrolling budget or establish peak-memory/source parity.

### Borrowed sidebar entries at `16900b67`

[Three optimized runs](interaction-stages-16900b67.json) report 6.78 ms first frame,
25.74 ms navigation and 3.44 ms scrolling. Navigation dispatch/render medians are
21.03/4.63 ms; scroll dispatch/render medians are 0.024/3.42 ms. Sidebar construction
no longer clones full file bodies or builds its lightweight entries twice. All 1,032
TUI tests and scroll integration pass, including differential full-cell and mouse-hit
comparisons against the public renderer. The remaining scroll-render cost still exceeds
the earlier paired pinned-Hunk budget. No source-ledger or peak-memory gate is claimed.

### Direct offscreen row append at `78520576`

[Three optimized runs](interaction-stages-78520576.json) report 6.37 ms first frame,
25.47 ms navigation and 3.25 ms scrolling. Navigation dispatch/render medians are
21.06/4.44 ms; scroll dispatch/render medians are 0.024/3.22 ms. Appending an exact
single offscreen geometry row directly removes its temporary vector allocation.
The difference is small and host activity remains uncontrolled. All 1,032 TUI tests,
scroll integration, Clippy and formatting pass. Scrolling parity and peak-memory
acceptance remain unproven; no ledger interval is newly completed by this change.

### Fresh paired interaction and peak RSS at `c453574b`

[All nine raw process outputs](interaction-paired-peak-c453574b.json) were captured in
three alternating native/main/stable rounds after builds and tests finished. Timing
aggregates use the nearest-rank median of each process's median, matching the source
percentile function. Native runs retain the diagnostic stage instrumentation. Other
host activity was uncontrolled; all source startup variability remains in the report.

| Measurement | Workdeck | Hunk main | Hunk stable |
| --- | ---: | ---: | ---: |
| First frame median, ms | 6.35 | 19.51 | 18.71 |
| Navigation median, ms | 24.42 | 46.13 | 47.45 |
| Scroll tick median, ms | 3.23 | 1.43 | 1.38 |
| Maximum process peak RSS across runs, bytes | 170754048 | 613482496 | 591872000 |

Each invocation was wrapped with macOS `/usr/bin/time -l`. The installed Darwin
`getrusage(2)` manual specifies `ru_maxrss` in bytes. These are process high-water
measurements over the complete invocation, not point-in-time RSS, JavaScript heapUsed,
or summed memory across a process tree. No peak-RSS regression appears in this workload;
that does not establish the complete memory suite's gate. Scrolling still exceeds the
10% latency budget against both pins. First frame is not full-process launch latency.

### Native refresh after highlighter lifecycle integration at `b13a20e8`

[Three raw optimized native samples](interaction-diagnostic-b13a20e8.json) were taken
in fresh sequential processes after the release runner build completed. Nearest-rank
medians of process medians are 6.56 ms first frame, 25.16 ms navigation and 3.18 ms
scrolling. Scroll rendering accounts for 3.16 ms. The workload remains 180 files,
120 lines per file and a 240 by 28 viewport; all raw distributions are retained.

Other host activity was uncontrolled and Hunk was not rerun alongside these samples.
These results identify rendering as the next scroll-cost profiling target, not a
causal speedup from annotation hashing or a fresh paired acceptance result. The old
paired scrolling failure remains unresolved. This run does not measure full launch,
reload, peak RSS, or the complete benchmark matrix; no ledger mapping changes.

### Empty-registry projection bypass at `7bddee4a`

[Three fresh native samples](interaction-diagnostic-7bddee4a.json), after rebuilding
the optimized runner, report medians of 6.45 ms first frame, 25.00 ms navigation and
3.22 ms scrolling, including 3.19 ms scroll rendering. The preceding native-only
sample was 3.18 ms scrolling: these uncontrolled measurements do not demonstrate a
speedup. Skipping unused projections with no highlighters is not sufficient to fix
the dominant render cost. Raw distributions remain recorded without normalization.
No fresh Hunk comparison, peak-RSS measurement or performance-gate pass is claimed.

### Cached gap descriptors at `3276f968`

[Three optimized native samples](interaction-diagnostic-3276f968.json) report
6.65 ms first frame, 25.52 ms navigation and 2.81 ms scrolling, with 2.78 ms in scroll
rendering. The preceding native sample was 3.22 ms scrolling. This is an encouraging
observation after eliminating repeated source-line counting, not a controlled causal
comparison: host activity remains uncontrolled and the Hunk pins were not rerun.
All raw distributions are retained. Initial cache construction adds a descriptor
pass, and navigation still constructs full geometry. The earlier paired scrolling
gate remains unresolved; these samples do not measure full launch, reload or peak RSS.

### Cached split-line plans at `1ed76935`

[Three optimized native samples with raw process output](interaction-diagnostic-1ed76935.json)
report medians of 6.75 ms first frame, 25.02 ms navigation and 2.64 ms scrolling,
including 2.61 ms scroll rendering. The preceding native-only run measured 2.81 ms
scrolling. Every process was wrapped with macOS `/usr/bin/time -l`; the maximum
process peak RSS across these three invocations was 170,328,064 bytes. This retains
the cost of the added split-index cache rather than measuring only live malloc usage.

The results are native-only and host activity was uncontrolled. They do not prove a
causal improvement, a fresh paired Hunk comparison, or the full memory acceptance
matrix. Scrolling still exceeds the older paired Hunk budget. Full launch and reload
are not measured; no ledger mapping or benchmark gate is completed.

### Split-row vector reservation at `ece40ecf`

[Three optimized native runs](interaction-diagnostic-ece40ecf.json) report medians
of 6.54 ms first frame, 24.74 ms navigation and 2.45 ms scrolling, with 2.40 ms in
scroll rendering. Maximum process peak RSS was 170,442,752 bytes. The preceding
native run measured 2.64 ms scrolling and 170,328,064 bytes maximum peak RSS.
Every raw process output is retained. Other host activity was uncontrolled, so
this is not a controlled causal comparison or a fresh paired Hunk gate. Scrolling
remains above the older paired source budget; full launch, reload and the complete
memory suite remain unverified.

### Frame capacity hint at `43bb5f4e`

[Three fresh optimized native runs](interaction-diagnostic-43bb5f4e.json) measured
6.40 ms first frame, 25.26 ms navigation and 2.50 ms scrolling, including 2.47 ms
scroll rendering. Maximum process peak RSS was 170,934,272 bytes. The preceding
sample was 2.45 ms scrolling and 170,442,752 bytes peak RSS. These uncontrolled
native-only samples show no demonstrated improvement from the frame-capacity hint.
Raw process output and all distributions are retained. No paired Hunk comparison,
complete memory-suite result, or performance-gate success is claimed.

### Default geometry-memory diagnostic at `c5760b7e`

[One raw optimized native sample](geometry-memory-native-c5760b7e.json) retains
10,260 geometry rows for 180 files before materializing their lazy plans. RSS
snapshots are 18,219,008 bytes after fixture construction, 46,809,088 after geometry
and 58,277,888 after row materialization. The separately constructed 50,000-line
giant fixture materialized 44,009 rows in 204.39 ms. This exposes retained-plan
and first-copy costs for follow-up work; it is one uncontrolled native diagnostic,
not peak RSS, JavaScript heap statistics, or a paired source acceptance result.

### Compact note-target lookup at `acaaf092`

[Three raw optimized native runs](interaction-diagnostic-acaaf092.json) report
6.14 ms first frame, 24.40 ms navigation and 2.27 ms scrolling, with 2.24 ms in
scroll rendering. Maximum process peak RSS was 172,392,448 bytes, versus
170,934,272 bytes in the preceding 2.50 ms scroll sample. Both the lower observed
latency and higher observed memory are retained; no metric is normalized away.
Host activity remains uncontrolled, and these native-only samples are not a causal
comparison, a paired Hunk acceptance run or a complete memory-suite result.
