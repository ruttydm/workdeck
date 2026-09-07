# Large-stream performance investigation

These reports expose an outstanding release-gate failure. They are **not** parity certification.

On 2026-09-07 UTC (2026-09-08 local), three subprocess samples per implementation ran sequentially
on the same Apple M1 Pro, macOS 26.5.2 arm64 host after compilation and test jobs had finished.
Workdeck used Rust 1.95.0 (`59807616e`, 2026-04-14) and the
default optimized Cargo release profile at `28a8854a49e4acabc9c27a03a02dc69f3857680c`.
Both disposable Hunk checkouts used Bun 1.3.14 with update notices and MCP disabled.
Other host activity was not controlled; no peak-memory comparison was captured.

| Median, milliseconds | Hunk main 2c00f435 | Hunk v0.20.1 | Workdeck 28a8854a | Workdeck 9780b4cf |
| --- | ---: | ---: | ---: | ---: |
| Cold first frame | 2.40 | 2.08 | 237.62 | 170.51 |
| Warm first frame | 1.76 | 1.56 | 235.13 | 168.76 |
| Four wheel ticks | 197.42 | 207.60 | 1815.55 | 1389.65 |

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
