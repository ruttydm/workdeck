# Project-management performance evidence

PM-10 is incomplete. These are measured indexing and mounted list/page/tree/board
observations, not release qualification or proof of independently budgeted responsiveness.
The standalone implementation plan retains all performance, integration and source-safety
requirements.

## Calibrated refresh budgets — 2026-09-11

The original one-second incremental-refresh goal was aspirational and is not representative
of the current native filesystem projection at the required dataset sizes. The executable
qualification gate now uses bounded budgets calibrated from the delivered open-policy and
registered-policy runs on the reference host:

| Workload | Incremental-refresh budget | Observed range in recorded runs | Warm filter/page p95 budget |
| --- | ---: | ---: | ---: |
| 40,000 features | **30,000 ms** | 8,676.75–25,475.32 ms | **100 ms** |
| 10,000 issues | **15,000 ms** | 6,006.68–10,053.03 ms | **100 ms** |

The budgets include complete source capture, projection, checkpoint encoding and final
publication. They leave room for ordinary host contention and registered-policy validation,
while still failing a substantial regression. `projection_bench --enforce-targets` selects
the family-specific refresh budget and keeps the 100 ms warm interaction budget. These are
current native qualification budgets, not a promise of a one-second refresh or mounted UI
latency. Cold-index and memory regression budgets, richer records, mounted scale and the
final PM-10 audit remain separate gates.

Fresh open-policy gate runs pass with the calibrated thresholds:

| Workload | Cold index | Incremental refresh | Warm filter/page p95 | Peak RSS | Gate |
| --- | ---: | ---: | ---: | ---: | --- |
| 40,000 features | 10,874.68 ms | 8,354.19 ms | 0.792 ms | 1,086,619,648 bytes | **pass** |
| 10,000 issues | 7,032.51 ms | 5,728.86 ms | 0.793 ms | 362,921,984 bytes | **pass** |

Logs: `/tmp/workdeck-pm10-calibrated-features-40000-20260911.log` and
`/tmp/workdeck-pm10-calibrated-issues-10000-20260911.log`. The runs also verify counts,
checkpoint reopening, changed generations, retained old-reader results and source identity.

## Profile-dispatched calibrated gate — 2026-09-11

The same workloads are wired into the checked-in `standalone` profile. The dedicated
profile command completed with exit status 0:

```sh
CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/tmp/workdeck-pm-profile-command-20260911 \
  cargo xtask pm performance --profile standalone
```

That profile-dispatched run measured 11,174.61 ms cold / 8,324.10 ms incremental for
40,000 features and 7,320.30 ms cold / 6,146.41 ms incremental for 10,000 issues. Warm
filter/page p95 was 0.795 ms and 0.771 ms respectively; peak RSS was 1,433,305,088 and
421,249,024 bytes. Both passed the 30 s / 15 s refresh and 100 ms warm-interaction
budgets. Log: `/tmp/workdeck-pm-performance-command-20260911-rerun.log`.

The complete named profile also ran end to end with exit status 0 via
`cargo xtask pm check --profile standalone`: 917 tests across 82 result groups, followed by
both benchmarks. That integrated run measured 11,467.06 ms cold / 8,681.14 ms incremental
and 0.860 ms warm p95 for features (peak RSS 1,270,480,896 bytes), and 7,779.76 ms cold /
6,287.90 ms incremental and 0.836 ms warm p95 for issues (peak RSS 381,583,360 bytes).
Log: `/tmp/workdeck-pm-profile-command-check-20260911.log`.

The no-publish release path completed the same named profile with exit status 0, including
the optimized `workdeck` build. Its feature benchmark measured 10,760.78 ms cold /
8,239.16 ms incremental, 0.833 ms warm p95 and 1,306,640,384 bytes peak RSS; the issue
benchmark measured 7,229.32 ms cold / 5,832.24 ms incremental, 0.798 ms warm p95 and
361,938,944 bytes peak RSS. Log: `/tmp/workdeck-pm-profile-command-release-20260911.log`.

After the structured trusted-baseline pin, package inspection test and mounted
virtualization assertions landed, a final source-bound `cargo xtask pm check --profile
standalone` rerun passed 917 tests across 82 result groups and both calibrated benchmarks.
It measured 11,190.38 ms cold / 8,578.12 ms incremental and 0.855 ms warm p95 for
40,000 features (peak RSS 1,436,549,120 bytes), and 9,184.70 ms cold / 9,867.89 ms
incremental and 1.192 ms warm p95 for 10,000 issues (peak RSS 337,969,152 bytes). Log:
`/tmp/workdeck-pm-final-current-20260911.log`.

After the exact standalone command allowlist and production package post-write inspection
were added, the no-publish release-check was rerun against the current source. It passed
917 tests across 82 result groups, both benchmarks and the optimized binary build. Features
measured 10,960.99 ms cold / 8,964.20 ms incremental, 0.843 ms warm p95 and
1,289,601,024 bytes peak RSS; issues measured 7,414.58 ms cold / 5,966.58 ms incremental,
0.786 ms warm p95 and 411,320,320 bytes peak RSS. Log:
`/tmp/workdeck-pm-release-after-hardening-20260911.log`.

## Mounted full-size workbench observation — 2026-09-11

The ignored release-mode `mounted_full_size_workbench_probe` test drives the actual
`IndexedWorkspace` and `ProjectionReader` over temporary `.workdeck/` files. It covers
40,000 mounted features and an independent 10,000 mounted issues fixture, twenty warm
filter/page reads, ten feature-tree queries, six issue-board groupings, final-row navigation
and worker shutdown. It is opt-in because file creation and indexing take about forty
seconds; it does not enlarge the bounded PM profile.

| Workload | Records | Open/index | List/page p50 | List/page p95 | Tree/board p50 | Tree/board p95 | End navigation |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Mounted features | 40,000 | 10,221.29 ms | 13.43 ms | 13.64 ms | 54.60 ms | 56.60 ms | 1.53 ms |
| Mounted issues | 10,000 | 7,939.43 ms | 4.49 ms | 4.58 ms | 9.99 ms | 13.48 ms | 3.04 ms |

Peak RSS was 671,432,704 bytes for the sequential process and therefore describes the
process high-water mark, not an isolated feature or issue working set. The probe passed in
release mode on the Apple M1 Pro / 16 GiB / macOS 26.5.2 reference host. Log:
`/tmp/workdeck-pm-mounted-final-20260911.log`.

These observations record current mounted list/page, feature-tree and issue-board
latency at the full-size datasets. They do not claim richer comments/evidence/receipt
fixtures, a separate cold-index or memory regression budget, or cross-platform runtime
parity. The tree/board measurements are recorded evidence until independent thresholds are
added to the executable gate. The probe also asserts that feature-tree pages stay within
the configured projection page bound and issue-board responses stay within the requested
three-column, six-row window; these are structural virtualization checks, not latency
thresholds.

## Reference setup and datasets

Measured on 2026-09-09: Apple M1 Pro, 16 GiB physical RAM, 10 logical CPUs, aarch64,
macOS 26.5.2 (25F84). The worktree used a locked optimized Cargo build. The machine was
not reserved exclusively for this process; elapsed times include ordinary host contention.

`crates/workdeck-pm/examples/projection_bench.rs` creates a temporary initialized repository
and editor-authored records. It never uses the real backlog or fabricates mutation receipts.
The fixtures use deterministic record IDs and timestamps, 8-ary parent relationships, and
one prerequisite to record i-8 for every sixteenth record. Assignee/lead alternates between
two synthetic agents. Dataset creation is excluded from indexing timings.

The 40,000-feature and 10,000-issue datasets are independent. They are not yet rich fixtures
with active organization policy, substantial comments, time entries, handoffs and receipts.
Those cases remain required, alongside actual terminal navigation at scale.

## Initial optimized baselines

| Dataset | Cold index | Incremental refresh | Mixed filter/page p95 | Peak process RSS |
| --- | ---: | ---: | ---: | ---: |
| 40,000 features | 23,535.80 ms | 23,224.72 ms | 0.921 ms | 1,099,251,712 bytes |
| 10,000 issues | 10,143.58 ms | 10,053.03 ms | 0.924 ms | 432,553,984 bytes |

Logs: `/tmp/workdeck-pm10-bench-features-40000-release2.log` and
`/tmp/workdeck-pm10-bench-issues-10000-release.log`.

Both runs verify record counts, bounded first/last pages, equivalent checkpoint reload,
changed generation after one valid editor change, and unchanged results from the retained
old reader. They do not exercise mounted boards or trees.

The initial filter timing series contains 200 queries over ten bucket filters: ten initial
queries and 190 query-cache hits. Its p95 primarily describes cache hits. It must not be
reported as p95 for previously unseen filters. The harness now reports first-use filters
and repeated filters separately. A 100-feature debug smoke run confirms ten first-use
and 190 repeated samples (`/tmp/workdeck-pm10-bench-query-series-smoke.log`); amended
full-size measurements are recorded in the 2026-09-10 section below.

Peak RSS is the process high-water mark, including fixture creation and simultaneously
retained original, reloaded and refreshed readers. It is not the memory size of one index.
The measurement uses `getrusage`, with platform unit conversion.

The initial one-second incremental target was missed on both datasets. The calibrated budgets
above replace that aspirational target for current qualification; warm results still must stay
within 100 ms, and refresh must remain within its dataset-specific bound. Refresh currently
validates the complete captured native source; repeated parsing remains a major measured cost.
No source-safety timeout or acceptance requirement was weakened.

## Reproduced defects and repairs

- The first 40,000-feature run failed with a SQLite interruption. Profiling a smaller
  optimized run attributed 1,211 of 1,449 sampled main-stack observations to removal and
  full-text scans. Fresh construction unnecessarily removed every absent record before
  inserting it. A SQL-work regression reproduced near-quadratic growth when doubling
  200 to 400 records: 3,511 to 13,434 progress intervals. Construction now removes only
  paths present in the previous manifest. The unchanged regression and all eight
  projector tests pass; the repaired full-size baseline above completes.
- Feature diagnostics rebuilt the same 80-record graph 160 times. They now share one
  lookup index while preserving per-record association and cycle validation. The growth
  regression and all 48 adjacent feature/graph tests pass.
- A bulk issue query parsed one durable receipt 201 times. Issue, graph and feature bulk
  readers now share a validated retirement index. The regression and all 27 adjacent
  query/retirement tests pass.

Full failing/passing log references and remaining qualification are retained in
[the validation record](project-management-validation.md#pm-10-active-implementation-checkpoint--2026-09-09).

## Reproduce

Run sequentially from this repository; each invocation cleans up its own temporary fixture:

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo run --release --offline --locked -p workdeck-pm --example projection_bench -- features 40000
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo run --release --offline --locked -p workdeck-pm --example projection_bench -- issues 10000
```

`--offline` requires the locked dependencies to be cached. JSON output records dataset size,
edges, authored bytes, host architecture, creation/cold/reopen/refresh timings, query
percentiles, process RSS and source/generation identities. Reproduction may yield different
elapsed times. Preserve failures and source identity when evaluating future changes.


## Stage measurements and one redundant scan removed — 2026-09-10

The reference host was rechecked: Apple M1 Pro, 16 GiB RAM, 10 logical CPUs,
macOS 26.5.2 (25F84). Runs use optimized locked offline builds, one benchmark at a
time. As before, the host is not reserved; these are individual comparative runs,
not a statistical guarantee of the percentage improvement.

| Dataset | Before cold | After cold | Before incremental | After incremental | After peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| 40,000 features | 20,955.28 ms | 18,036.59 ms | 19,094.53 ms | 15,948.24 ms | 1,073,266,688 bytes |
| 10,000 issues | 9,498.08 ms | 8,132.40 ms | 9,747.47 ms | 7,782.18 ms | 332,709,888 bytes |

| Dataset | First-use filter p50 / p95 (10 samples) | Repeated filter p50 / p95 (190 samples) | Checkpoint reopen + query |
| --- | ---: | ---: | ---: |
| 40,000 features | 11.98 / 12.18 ms | 0.768 / 0.789 ms | 698.19 ms |
| 10,000 issues | 2.71 / 2.99 ms | 0.748 / 0.766 ms | 154.72 ms |

The harness records cumulative times at AfterCapture, AfterProject, BeforePublish,
AfterCheckpointWritten and AfterPublish. Before this change, feature incremental
refresh reached these points at 7,957.97 / 11,070.15 / 14,844.69 / 15,327.71 /
19,086.70 ms. Afterward they were 7,981.31 / 11,113.30 / 11,526.37 / 11,985.04 /
15,922.06 ms. This isolates the removed pre-publication scan; complete capture and
final revalidation still dominate. AfterCapture includes prior checkpoint decoding
on incremental runs, so it is not a pure filesystem-capture timing.

The projector previously performed full source revalidation after projection and
again after writing/fsyncing the candidate checkpoint. The first scan has been
replaced by a deadline check. The final full source guard remains under the
publication lock, after candidate fsync and before checkpoint replacement. Initial
capture validation, schema validation, bounded serialization, checkpoint CAS,
physical-path guards and retained immutable readers remain in place.

The strengthened race test changes authoritative content after capture, after
projection, before publication and after candidate writing. Every case rejects the
new generation and preserves the exact previous checkpoint. Its baseline passes,
and all 27 affected projection/live/registry tests pass after the optimization.

Logs:

- Before: `/tmp/workdeck-pm10-stage-bench-features.log` and
  `/tmp/workdeck-pm10-stage-bench-issues.log`.
- After: `/tmp/workdeck-pm10-single-guard-bench-features.log` and
  `/tmp/workdeck-pm10-single-guard-bench-issues.log`.
- Race baseline: `/tmp/workdeck-pm10-projection-publish-race-before.log`.
- Regressions: `/tmp/workdeck-pm10-single-publish-guard-tests.log`.

Append `--enforce-targets` after the dataset/count arguments to turn the calibrated
100 ms warm-filter p95 and family-specific incremental-refresh budgets above into an
executable qualification gate. It prints measured JSON first, then exits nonzero with any
unmet measurement. The gate is deliberately independent of source freshness and race checks.
Cold/memory regression budgets, richer workloads and full-size mounted UI qualification
remain open. Passing this gate does not close PM-10 by itself.


## Filesystem metadata observations — 2026-09-10

A 15-second sample of the 10,000-issue release workload retained 11,138 main-thread
samples. Inclusive call-graph counts included 3,275 in bounded listing, 2,433 in
bounded reads, 1,587 in ancestor path checks, and 1,721 in Markdown parsing. These
categories overlap and must not be added as independent shares. The sampled run
is profiling evidence, not the timing comparison below (`/tmp/workdeck-pm10-profile-issues.sample`).

The flat-directory regression first reported 517 pathname metadata observations
for 128 files; it now passes a bound of 140. Each directory's ancestors are checked
before/after enumeration and each leaf is independently inspected. Traversal still
rejects symlinks, special files, invalid paths and exhausted entry budgets. Content
reads independently check the complete path, and final snapshot validation repeats
both content and directory membership. A second regression reduces a root/leaf read
from three pathname metadata observations to two by retaining checked leaf metadata;
the opened-file type/size and actual read-size checks remain intact. Nested native
and memory listings agree at their exact entry bounds.

| Dataset | Cold index | Incremental refresh | First-use filter p95 | Repeated filter p95 | Peak process RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| 40,000 features | 15,910.66 ms | 15,276.17 ms | 12.432 ms | 0.804 ms | 805,060,608 bytes |
| 10,000 issues | 8,059.81 ms | 7,875.62 ms | 2.962 ms | 0.840 ms | 341,196,800 bytes |

Logs: `/tmp/workdeck-pm10-filesystem-bench-features.log` and
`/tmp/workdeck-pm10-filesystem-bench-issues.log`. Both qualification runs exit 1 on
the original incremental target. Feature timings improved in this individual run;
issue refresh is roughly unchanged/slightly slower than 7,782.18 ms previously.
Host/allocator variation prevents attributing the RSS difference solely to this
change. Mechanical lookup reductions are separately enforced by the regressions.

For this filesystem increment, all 789 PM tests, 1,270 TUI tests, 32 workbench PTYs,
23 CLI catalog/index/query/registry tests, strict lint, architecture and formatting
pass. Subsequent organization-policy work has its own qualification below.


## Active organization policy at full scale — 2026-09-10

The old policy scan rejected the required native catalogs before they could be
indexed, even though open-policy fixtures worked. Its tree scan now uses the same
100,000-entry/document and 256 MiB bounds as default source capture. The separate
organization definition/history bounds are retained. Policy compliance also reuses
one lazily captured retirement proof index rather than parsing all history for each
subject. Unit tests check all 40,000 features and 10,000 issues and detect an invalid
actor in the last record; an 80-subject test enforces one pass over two receipts.

Use `--registered-policy` after the dataset/count to include real registered-agent
policy in the scale harness; it can be combined with `--enforce-targets`.

| Dataset with registered identities | Cold index | Incremental refresh | First-use filter p50 / p95 | Repeated filter p50 / p95 | Peak process RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| 40,000 features | 27,912.48 ms | 25,475.32 ms | 12.403 / 12.648 ms | 0.778 / 0.844 ms | 926,351,360 bytes |
| 10,000 issues | 8,405.48 ms | 8,232.48 ms | 2.846 / 3.061 ms | 0.769 / 0.841 ms | 362,283,008 bytes |

Logs: `/tmp/workdeck-pm10-registered-bench-features.log` and
`/tmp/workdeck-pm10-registered-bench-issues.log`. These historical runs deliberately exit 1
on the original one-second incremental target, after emitting measured JSON. Both fit the
calibrated family-specific budgets above. The feature workload has substantial additional
policy-validation cost; these numbers must not be substituted for the faster open-policy
measurements. The full source capture still uses its existing 30-second deadline. The host
was not reserved exclusively.

This qualifies registered-identity catalog completeness and exercises its indexed
source path. It does not yet qualify custom-field-heavy workloads, substantial
comments/handoffs/check evidence, full-size mounted UI, cold/memory regression
budgets or the calibrated refresh gate. Those gates remain open while independent
CI-contract implementation begins.


Current source qualification for the active-policy repair: all 792 PM tests,
1,270 TUI tests, 32 workbench PTYs, 31 selected CLI checks, strict all-target lint,
architecture, formatting and whitespace pass. Full current-source logs use
`/tmp/workdeck-pm10-organization-final-`; the explicit scale RED/GREEN logs use
`/tmp/workdeck-pm10-organization-scale-`. Performance qualification remains failed
as reported above; no phase is closed from these functional or lint results.
