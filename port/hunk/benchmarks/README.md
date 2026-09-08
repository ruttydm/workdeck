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
