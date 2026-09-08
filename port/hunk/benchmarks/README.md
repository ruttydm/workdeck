# Review-stream performance investigations

These reports expose an outstanding release-gate failure. They are **not** parity certification.

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
selection changes, viewport movement and finite timing samples. It is currently test-only:
native retained RSS/heap measurements and same-host optimized comparisons remain missing.
Frozen single-run output from both pinned anchors is now recorded in
[the interaction oracle](../oracles/benchmark-interaction-latency.json), covering all 13 metric
names and source-scale counts. Those sequential original-runtime captures are diagnostic evidence,
not repeated performance acceptance. The source ledger record remains unmapped and the default runner
still rejects `interaction-latency.ts`; this partial test is not benchmark parity evidence.
