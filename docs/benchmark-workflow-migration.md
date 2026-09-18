# Benchmark workflow migration

The pinned Hunk `.github/workflows/benchmarks.yml` ran four benchmark scripts
through Bun, wrote one text file per workload, appended a step summary, and
uploaded the results. Workdeck keeps the same trigger, concurrency, workload,
summary, and artifact contract in the native workflow, but invokes the Rust
owners directly:

- `cargo xtask benchmark bootstrap-load`
- `cargo xtask benchmark highlight-prefetch`
- `cargo xtask benchmark large-stream`
- `cargo xtask benchmark wrapped-cjk`

The workflow uses the pinned Rust toolchain/cache actions and does not install
or execute Bun, Node, npm, OpenTUI, or a Hunk runtime. The pinned file is read
from the protected baseline by `benchmark::verify_workflow`; the original Bun
workflow is not executed. Native benchmark outputs remain explicit evidence,
and the release gate compares them with the frozen Hunk reports rather than
claiming parity from a passing CI job alone.
