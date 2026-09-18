# Huge review interaction diagnostic

Run the native diagnostic with:

```console
cargo xtask benchmark huge-stream-diagnostic
```

The release-runner form is also available through the pinned workload name and
emits aggregated `METRIC` lines suitable for `benchmark run`:

```console
cargo xtask benchmark run --script huge-stream.ts --samples 1
```

It uses the same single-renderer interaction sequence. RSS is the portable
memory metric; macOS additionally reports allocator-in-use bytes when that
counter is available. Those native counters are intentionally not described as
JavaScript heap usage.

This opt-in workload uses the translated Hunk huge fixture: 1,000 files with
300 lines each plus one 50,000-line file. It builds one renderer, measures the
first frame, renders two startup-settling frames, executes six wheel ticks,
then four next-hunk key presses. It checks that scrolling advances and every
key press changes the actual review selection. Fixture construction is timed
separately. Unoptimized execution can be slow; this is not part of default tests.

Output is diagnostic JSON with individual interaction samples, first-frame and
fixture-build times, and current native memory after first frame and navigation.
`rendererSetupMs` separately times review/renderer construction, outside the
first-frame interval. `beforeRenderer` and `afterRenderer` capture current memory
around that construction, while `peakBeforeRendererBytes` and
`peakAfterRendererBytes` capture lifetime high-water marks at those boundaries.
The before-renderer boundary already includes fixture construction. These peaks
are cumulative, not isolated phase allocations; their difference cannot measure
all allocations made by a phase. Snapshot collection is outside the setup and
first-frame timing intervals. Older reports do not contain these additive fields.
The current RSS/malloc snapshots are not JavaScript heap usage or peak RSS.
`peakProcessRssBytes` separately records the process-lifetime resident/working-set
high-water mark, including fixture construction. macOS reports bytes directly;
Linux KiB are checked and converted to bytes; Windows uses peak working set.
This is not a GC-retained heap measurement or a per-stage peak. Unit conversions
and the live macOS backend have tests; Linux and Windows need their native CI runs. The
command remains outside the parity-gated release runner: dual-pin differential
execution, full timing/lifecycle equivalence and release acceptance remain open.

The first complete native debug run is retained in
[`huge-stream-native-diagnostic.json`](../port/hunk/huge-stream-native-diagnostic.json),
with source hashes and workload counts. It completed all 1,001 files and the
specified interactions; it does not replace same-host paired acceptance runs.
The subsequent [peak-enabled sample](../port/hunk/huge-stream-native-peak-diagnostic.json)
records both the lifetime high-water mark and current snapshots separately.

Both pinned Hunk workloads have now been executed without changing their source:
[`benchmark-huge-stream.json`](../port/hunk/oracles/benchmark-huge-stream.json)
retains all 15 metrics from each run, runtime identity and source/helper blob
identities. A Rust test validates workload counts and metric validity. These
single runs do not establish repeatability or comparable native/source build
profiles, and Hunk's source command does not emit process peak RSS.

A subsequent external `/usr/bin/time -l` capture supplies source process peaks:
[`benchmark-huge-stream-process-peak.json`](../port/hunk/oracles/benchmark-huge-stream-process-peak.json).
On this Darwin arm64 host, maximum resident set size was 872,185,856 bytes
for main and 902,791,168 bytes for stable. The retained native debug sample's
1,385,152,512-byte peak exceeds both observations. This is not a repeated,
release-profile acceptance comparison and must not be reported as passing.
The fixture test distinguishes resident peak from macOS's separate peak memory
footprint field and checks the recorded current RSS does not exceed its peak.

The small-fixture executable regression checks the same interaction sequence
using six ordinary files, not the huge workload's performance. Fixture content
has separate pinned-source checks in `xtask/src/benchmark/stream.rs`. Renderer
construction is shared with the existing large-stream diagnostic.

Native Rust/Ratatui reimplementation of Hunk `benchmarks/huge-stream.ts`, MIT, Copyright (c)
Modem Labs Inc. The source is verified at both pins by `cargo xtask verify` and the strict port
audit; no TypeScript runtime is retained. The pinned source is 2,388 bytes with SHA-256
`af90c5429effc6a9c26b69e6db09e86fa4aacb2af9addbbfd1eb0246e17021e5`.
