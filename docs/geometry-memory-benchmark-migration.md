# Geometry-memory benchmark migration

The pinned Hunk workload `benchmarks/geometry-memory.ts` is identical at the
baseline (`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`) and stable
(`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`) pins: 6,932 bytes, 194 lines, and
SHA-256 `5a3707a75a0b730392016642d1d28ee6be5b9c04eef8e0870947055318e9f5c8`.

`cargo xtask benchmark geometry-memory` uses the native Rust geometry cache for
the retained geometry workload
and the same moderate all-files fixture, then materializes the deferred plan
used by copy selection and measures a separate giant-file plan. It preserves
the source option grammar (`--file-count`, `--lines-per-file`, `--width`, and
`--no-gc`), clamping and error ordering, lazy-plan invariant, row counts, and
metric names. The native report adds exact RSS and allocator snapshots where
the host exposes them; JavaScript heap/object counters are not fabricated, and
there is no JavaScript heap in the native process.

The benchmark is diagnostic rather than an acceptance waiver. It owns all
temporary data, never writes repository state, and runs without a JavaScript
runtime. The dual-pin verifier reads the source through Git, checks its exact
bytes, line count, hash, and markers, and requires executable Rust tests for
the option, geometry, lazy materialization, and giant-file contracts.
