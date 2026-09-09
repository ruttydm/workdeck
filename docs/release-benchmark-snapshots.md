# Release benchmark snapshots

Version benchmark reports so a release can be audited after publication. Active
Workdeck reports belong at `benchmarks/release/bench-x.y.z.json`; the pinned Hunk
reports are separately retained in the [historical archive](historical-release-benchmarks.md).
Never relabel those upstream measurements as Workdeck measurements.

## Release preparation

Before a release tag, capture benchmarks for the `workdeck-cli` Cargo package
version, retain the report with the release-preparation commit, and compare it
against the latest lower stable release snapshot:

```console
cargo xtask benchmark release-plan
cargo xtask benchmark compare-release
```

`release-plan` resolves release capture options; it does **not** execute the
complete suite. Individual implemented workloads can be run with
`cargo xtask benchmark run --script WORKLOAD --samples N --out PATH`.
The complete native release-suite capture remains unfinished, so individual
workload reports or debug diagnostics are not substitutes for its final artifact.

The comparison command defaults to the current Cargo version and
`benchmarks/release/`; it fails when the current report or a lower stable baseline
is missing. Explicit paths are available through `--head`, `--base`, and
`--release-dir`. A final release pipeline must require the report and passing
comparison before publishing binary/package artifacts. The port's wider release
gates remain incomplete. Workdeck does not publish npm packages, and these
commands do not authorize tagging, pushing, or publishing.

## Historical regression policy and strict port acceptance

The translated historical comparator uses medians and fails regressions only
when they exceed **both** relative and absolute thresholds recorded in report
metadata. Its defaults are +15% and +5 ms for timing, and +20% and +8 MiB for
memory. New metrics are informational until a later release has a baseline;
missing previously comparable metrics fail because that measurement is no
longer protected.

Historical reports can carry metric names and rationale in `acceptedRegressions`.
The comparator marks those rows accepted rather than failing, retaining the
tradeoff's audit trail. This describes the translated historical comparator,
not an exemption for the Workdeck–Hunk semantic port: completion requires no
more than 10% launch/reload/navigation/render latency regression and no peak-memory
regression on the same host. Accepted-regression entries cannot waive that gate.

## Backfilling

Generate a missing historical baseline from its actual release tag, using the
same runtime version and runner class as the corresponding historical capture.
Executing pinned Hunk for this purpose is permitted only in a disposable oracle
checkout; do not add its runtime or TypeScript source to the Workdeck repository.
For native Workdeck reports, retain the exact source revision, Rust toolchain,
build profile, host class and workload settings so comparisons are reproducible.
Commit the backfilled snapshots before relying on comparisons against them.
Do not fabricate missing measurements or derive them from another version.

Adapted from pinned Hunk `benchmarks/release/README.md`, MIT, Copyright (c)
Modem Labs Inc. This document migrates its reporting, preparation, threshold,
acceptance-history and backfill guidance; it does not complete the separately
tracked benchmark runner or release workflow implementations.
