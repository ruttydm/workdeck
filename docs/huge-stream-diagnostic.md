# Huge review interaction diagnostic

Run the native diagnostic with:

```console
cargo xtask benchmark huge-stream-diagnostic
```

This opt-in workload uses the translated Hunk huge fixture: 1,000 files with
300 lines each plus one 50,000-line file. It builds one renderer, measures the
first frame, renders two startup-settling frames, executes six wheel ticks,
then four next-hunk key presses. It checks that scrolling advances and every
key press changes the actual review selection. Fixture construction is timed
separately. Unoptimized execution can be slow; this is not part of default tests.

Output is diagnostic JSON with individual interaction samples, first-frame and
fixture-build times, and current native memory after first frame and navigation.
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

The small-fixture executable regression checks the same interaction sequence
using six ordinary files, not the huge workload's performance. Fixture content
has separate pinned-source checks in `xtask/src/benchmark/stream.rs`. Renderer
construction is shared with the existing large-stream diagnostic.

Partial translation of Hunk `benchmarks/huge-stream.ts`, MIT, Copyright (c)
Modem Labs Inc. The source ledger record remains unmapped.
