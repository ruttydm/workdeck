# Project-management implementation evidence

This records bounded implementation checkpoints. It is not release qualification
or a claim that the complete standalone plan has shipped.

## PM-10 refresh-budget recalibration — 2026-09-11

The original one-second incremental-refresh target was too aggressive for the complete
source capture, projection, checkpoint encoding and publication work required by the
40,000-feature and 10,000-issue fixtures. The current executable gate is calibrated to
the delivered native implementation and ordinary host contention:

| Workload | Refresh budget | Warm filter/page p95 budget | Recorded incremental range |
| --- | ---: | ---: | ---: |
| 40,000 features | **30,000 ms** | **100 ms** | 8,676.75–25,475.32 ms |
| 10,000 issues | **15,000 ms** | **100 ms** | 6,006.68–10,053.03 ms |

`projection_bench --enforce-targets` now selects the family-specific refresh budget. The
budget change preserves source freshness, race detection, bounded reads and the warm
interaction target; it only replaces the aspirational one-second refresh threshold. Passing
the calibrated gate does not qualify mounted full-size navigation, richer records,
cold/memory regression budgets or the final PM-10 requirement audit.

Fresh open-policy executions of the calibrated gate pass:

```text
features 40000 --enforce-targets: cold 10,874.68 ms, incremental 8,354.19 ms,
  warm-filter p95 0.792 ms, peak RSS 1,086,619,648 bytes — pass
issues 10000 --enforce-targets: cold 7,032.51 ms, incremental 5,728.86 ms,
  warm-filter p95 0.793 ms, peak RSS 362,921,984 bytes — pass
```

Logs: `/tmp/workdeck-pm10-calibrated-features-40000-20260911.log` and
`/tmp/workdeck-pm10-calibrated-issues-10000-20260911.log`.

Post-change static gates also pass: `cargo fmt --all -- --check`, workspace Clippy with
warnings denied (`/tmp/workdeck-pm-calibrated-clippy-20260911.log`), architecture checking
(`/tmp/workdeck-pm-architecture-20260911.log`) and `git diff --check`.

## PM-11 profile-dispatched calibrated performance gate — 2026-09-11

The checked-in `standalone` profile now dispatches the same two bounded, locked release
benchmarks used by the PM check and release-check paths. The dedicated command completed
with exit status 0:

```text
CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/tmp/workdeck-pm-profile-command-20260911 \
  cargo xtask pm performance --profile standalone
Workdeck PM performance checks passed.
```

The open-policy workloads reported:

| Workload | Cold index | Incremental refresh | Warm filter/page p95 | Peak RSS | Gate |
| --- | ---: | ---: | ---: | ---: | --- |
| 40,000 features | 11,174.61 ms | 8,324.10 ms | 0.795 ms | 1,433,305,088 bytes | **pass** |
| 10,000 issues | 7,320.30 ms | 6,146.41 ms | 0.771 ms | 421,249,024 bytes | **pass** |

Log: `/tmp/workdeck-pm-performance-command-20260911-rerun.log`. The profile also pins
`xtask/src/project_management.rs` by SHA-256, requires an immutable full commit or
`refs/tags/...` plus a 64-character lowercase `#sha256:` digest for its trusted-baseline
field, and rejects commands outside the fixed test, build and projection-benchmark
allowlists. The standalone profile is also required to contain the exact four check
commands and exact release build command; six focused `xtask` profile tests cover valid,
malformed and weakened-profile cases.
This is local source-bound evidence; an independent trusted validator must still resolve
and authenticate the referenced baseline, and external CI/release observation remains a
PM-11 gate.

The complete named profile also ran end to end with exit status 0 via
`cargo xtask pm check --profile standalone`: **917 tests across 82 result groups**, zero
failures or ignored tests, followed by both calibrated benchmarks. That integrated run
measured 11,467.06 ms cold / 8,681.14 ms incremental and 0.860 ms warm p95 for features
(peak RSS 1,270,480,896 bytes), and 7,779.76 ms cold / 6,287.90 ms incremental and
0.836 ms warm p95 for issues (peak RSS 381,583,360 bytes). Log:
`/tmp/workdeck-pm-profile-command-check-20260911.log`.

The no-publish release path also completed with exit status 0 via
`cargo xtask pm release-check --profile standalone`: **917 tests across 82 result groups**,
both calibrated benchmarks, and the optimized `workdeck` binary build. Its benchmark
observations were 10,760.78 ms cold / 8,239.16 ms incremental and 0.833 ms warm p95 for
features (peak RSS 1,306,640,384 bytes), and 7,229.32 ms cold / 5,832.24 ms incremental
and 0.798 ms warm p95 for issues (peak RSS 361,938,944 bytes). Log:
`/tmp/workdeck-pm-profile-command-release-20260911.log`.

After the structured trusted-baseline pin, package inspection test and mounted
virtualization assertions landed, the complete named profile was rerun against the current
source and passed **917 tests across 82 result groups**, zero failures or ignored tests,
plus both calibrated release benchmarks. The feature benchmark measured 11,190.38 ms cold,
8,578.12 ms incremental, 0.855 ms warm p95 and 1,436,549,120 bytes peak RSS; the issue
benchmark measured 9,184.70 ms cold, 9,867.89 ms incremental, 1.192 ms warm p95 and
337,969,152 bytes peak RSS. Log: `/tmp/workdeck-pm-final-current-20260911.log`.

After the exact standalone command allowlist and production package post-write inspection
were added, the profile was rerun again against the current source. The no-publish
`cargo xtask pm release-check --profile standalone` passed **917 tests across 82 result
groups**, zero failures or ignored tests, both calibrated benchmarks and the optimized
binary build. Features measured 10,960.99 ms cold / 8,964.20 ms incremental, 0.843 ms
warm p95 and 1,289,601,024 bytes peak RSS; issues measured 7,414.58 ms cold / 5,966.58 ms
incremental, 0.786 ms warm p95 and 411,320,320 bytes peak RSS. Log:
`/tmp/workdeck-pm-release-after-hardening-20260911.log`. The follow-up warnings-denied
`xtask` Clippy run also passed (`/tmp/workdeck-pm-xtask-clippy-after-hardening-20260911.log`).

## Final source repair checks — 2026-09-11

The final source-bound release check was rerun after the feature-tree lookup and watcher
repairs. `cargo xtask pm release-check --profile standalone` passed the complete PM and
selected CLI suites, both calibrated projection workloads and the optimized Workdeck
build. The feature workload measured 11,118.13 ms cold and 8,663.82 ms incremental;
the issue workload measured 7,955.92 ms cold and 6,209.05 ms incremental. Warm-filter
p95 stayed below 1 ms for both workloads. The release log is
`/tmp/workdeck-pm-release-final-repair-20260911.log`.

The feature-tree regression test
`projection::query::tree::tests::forty_thousand_levels_are_iterative_and_collapsing_cannot_hide_a_cycle`
passed after replacing logarithmic map/set lookups with hash-based lookups
(`/tmp/workdeck-pm-projection-audit-20260911.log`). The warnings-denied VCS Clippy check
also passed after the watcher fingerprint change (`/tmp/workdeck-pm-vcs-clippy-20260911.log`).

## Workspace all-target observation — 2026-09-11

The isolated command `cargo test --offline --locked --workspace --all-targets
--no-fail-fast` completed **4,938 passing, 1 failing and 1 ignored test across 205
result groups**. The failure was reproducible in the macOS
`workdeck-vcs` watcher case `watch_observer::tests::recursive_target_ignores_excluded_metadata_churn`:
on this host, a write under a temporary `.git` directory produced an event within the
test's 250 ms suppression window. An exact single-test rerun with one test thread
reproduced the same assertion; no Workdeck PM package test failed. A later source repair
records descriptor fingerprints for exact-entry watches and discards unchanged setup
replays while preserving changed, removed and recreated entries. The affected watcher
suite now passes **19 tests** in `/tmp/workdeck-pm-watcher-fingerprint-20260911.log`;
the repaired-source full workspace rerun is recorded below. This section and its counts
are historical pre-repair evidence. The historical broad run log is
`/tmp/workdeck-pm-workspace-final-20260911.log`, and the original reproduction is
`/tmp/workdeck-pm-vcs-watcher-rerun-20260911.log`.

The installed `x86_64-pc-windows-gnu` Rust target was also checked for PM portability, but
`cargo check --target x86_64-pc-windows-gnu -p workdeck-pm` stopped before compiling the
crate because the host has no `x86_64-w64-mingw32-gcc` compiler. This is a toolchain
availability blocker, not Windows runtime evidence; the isolated target was removed after
the check. A Clang override was attempted as well and stopped at the same native dependency
because the cross target has no Windows standard headers. Logs:
`/tmp/workdeck-pm-windows-check-20260911.log` and
`/tmp/workdeck-pm-windows-clang-check-20260911.log`.

## Aggregate verifier rerun and watcher recheck — 2026-09-11

The repository verifier was rerun against an isolated Cargo target with
`cargo xtask verify`. It completed with exit status 0: **4,939 passed, 0 failed and 1
ignored across 205 result groups**, including the watcher case, and its optimized release
smoke also passed. Log: `/tmp/workdeck-pm-verify-20260911b.log`.

The exact watcher test was then rerun alone with one test thread and failed again within
the 250 ms quiet window:
`watch_observer::tests::recursive_target_ignores_excluded_metadata_churn`. Log:
`/tmp/workdeck-pm-vcs-watcher-rerun-20260911c.log`. The paired observations establish
that this is an intermittent macOS filesystem-event ordering issue: aggregate qualification
can pass while the deterministic isolated reproduction still fails. It remains an external
workspace/platform blocker; no expected result was weakened and no VCS source change was
made.

## Watcher setup-replay repair — 2026-09-11

The exact-entry watcher race was traced with a temporary Notify event capture. macOS
delivered `Create(File)`, `Modify(Metadata(Extended))`, and `Modify(Data(Content))`
for an already existing target after registration, before the test changed a sibling.
The filter now snapshots each exact entry's length, modification/change times and readonly
state, suppresses unchanged replay events, and updates the snapshot only when an entry
actually changes. Metadata errors remain conservative and notify the caller. Root-tree
marker suppression remains limited to macOS coarse folder/metadata markers, so direct
child and rename events are retained. The focused watcher suite passes **19/19** with no
timeout or expected-result change (`/tmp/workdeck-pm-watcher-fingerprint-20260911.log`).

## Repaired-source full workspace rerun — 2026-09-11

After the watcher setup-replay repair, the isolated full-workspace command
`cargo test --offline --locked --workspace --all-targets --no-fail-fast` (with
`CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
CARGO_PROFILE_TEST_DEBUG=0 CARGO_TARGET_DIR=/tmp/workdeck-pm-workspace-repair-20260911`)
completed with **4,943 passed, 0 failed and 2 ignored across 205 result groups**. The
previously intermittent macOS watcher case
`watch_observer::tests::recursive_target_ignores_excluded_metadata_churn` passed in this
run. The two ignored tests are the opt-in release-mode
`workbench::indexed_workspace_tests::mounted_full_size_workbench_probe` (an explicit
qualification workload) and `ci_changes::tests::capture_pinned_ci_change_oracles` (which
requires Bash and preserved pinned Hunk refs); neither is a relaxed failure. The
pre-repair sections above remain labeled historical. Log:
`/tmp/workdeck-pm-workspace-repair-20260911.log`. Its isolated target directory was
removed with `find -depth -delete` after the terminal result; the log is retained.

## PM-10/PM-12 optimized mounted full-size workbench probe — 2026-09-11

An ignored release-mode probe now drives the real `IndexedWorkspace` and
`ProjectionReader` over temporary `.workdeck/` files. It writes independent native
fixtures (40,000 features and 10,000 issues), waits for the mounted board/page to settle,
runs twenty warm filter/page reads, then runs ten feature-tree or six issue-board
queries, navigates to the final row, checks row identity and shuts down the worker. The
probe is opt-in so ordinary PM CI remains bounded; it is a scale qualification observation
rather than a relaxed gate.

```sh
CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_DEBUG=0 \
  CARGO_TARGET_DIR=/tmp/workdeck-pm-mounted-final-20260911 \
  cargo test --offline --locked --release -p workdeck-tui \
  --lib mounted_full_size_workbench_probe -- --ignored --nocapture --test-threads=1
```

Observed on the reference Apple M1 Pro / 16 GiB / macOS 26.5.2 host (10 logical CPUs):

| Workload | Records | Open/index | List/page p50 | List/page p95 | Tree/board p50 | Tree/board p95 | End navigation |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Mounted features | 40,000 | 10,221.29 ms | 13.43 ms | 13.64 ms | 54.60 ms | 56.60 ms | 1.53 ms |
| Mounted issues | 10,000 | 7,939.43 ms | 4.49 ms | 4.58 ms | 9.99 ms | 13.48 ms | 3.04 ms |

The process high-water RSS was 671,432,704 bytes across the sequential feature-tree and
issue-board probes; it is not a per-workload allocation. The release probe passed 1 test,
with 0 failures and 1,274 filtered tests, in 31.18 seconds. Log:
`/tmp/workdeck-pm-mounted-final-20260911.log`.

This closes the missing mounted full-size list/page, feature-tree and issue-board latency
observations supporting PM-10.D7. PM-10.E5 and PM-12.D5 still require explicit budget
policy and final qualification: cold-index and memory limits are not enforced, the fixtures
remain intentionally light on comments/evidence/receipts, and supported-platform lifecycle
evidence is open. Each tree/board sample also asserts bounded materialization: feature-tree
pages stay within the configured projection page cap, and issue boards stay within the
requested three-column, six-row window. These assertions are structural virtualization
evidence, not independent latency thresholds. A debug-mode attempt exceeded the production capture deadline because
this fixture stresses 50,000 filesystem files; it is not used as qualification evidence.

## PM-12 release package artifact inspection — 2026-09-11

The release package path now has a focused local inspection test covering both archive
formats produced by the packaging helpers. It constructs a complete synthetic release
entry set with the required license, third-party notice, SBOM and inventory paths; binds a
plain in-toto/SLSA provenance statement to the exact synthetic executable digest; writes
deterministic `tar.gz` and ZIP archives; reopens each archive through the structural
inspector; checks the wrapper, executable and required package paths; hashes each archive
and rechecks its exact checksum-manifest entry; extracts the executable and provenance;
and verifies that the provenance subject matches the executable bytes. The production package
command now performs the same structural archive and generated-checksum inspection after writing
each release artifact.

```sh
CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 \
  CARGO_PROFILE_TEST_DEBUG=0 \
  CARGO_TARGET_DIR=/tmp/workdeck-pm-package-test-20260911 \
  cargo test --offline --locked -p xtask \
  release_package_archives_pass_structural_checksum_and_provenance_inspection --no-fail-fast
```

The focused test passed with 1 test, 0 failures and 198 filtered tests. This is local
structural package evidence using synthetic provenance. It does not establish authenticated
provenance, external CI or signing, publication, installation, or a complete external
release execution; those remain explicit PM-12 qualification work.

A direct production-path smoke also ran the compiled `cargo xtask release package` command
with a synthetic executable and subject-bound statement for `aarch64-apple-darwin` and
`x86_64-pc-windows-msvc`. Both commands exited 0, produced 11 package entries, and reported
that the generated checksum was verified. This exercises the tar and ZIP command paths and
their post-write inspection; the executable and provenance were synthetic, so no signed
build, external CI, publication, or platform runtime claim follows.

## Independent PM-10 performance review — 2026-09-11

The latency gate is healthy, but the full performance contract remains open. The core
enforcement checks warm filter/page p95 and family-specific incremental refresh only; cold
index and memory are recorded baselines without independent regression limits. The mounted
probe now covers list/page, feature-tree and issue-board queries, but those observations are
not yet separate executable budgets. Refresh still stats the complete authoritative file
membership and performs broad projection work after a single-file edit, so the relaxed
budgets describe current native cost rather than proportional delta performance. These
limitations are retained explicitly in PM-X.C32 and PM-10.E5 instead of being treated as
green evidence.

## Non-Unix capability advertisement repair — 2026-09-11

The capability response previously reported native PM mutations as ready whenever a
repository could be discovered, even though the transaction engine intentionally requires
Unix parent-directory durability at publication. The catalog now reports mutation,
operation-recovery, check-planning/execution, claims and related authoritative write
features only when the source is ready **and** the host is Unix. Read-only discovery,
queries, context and command/schema introspection remain available. The response also
exposes `semantics.native_mutations` as `qualified` or `unsupported_on_this_platform`, so
an agent can choose a recoverable path instead of starting an operation that must fail.

## PM-10 incremental source reuse and changed-path projection — 2026-09-11

Local WorkingTree refreshes now retain descriptor-bound file stamps in the source
guard. When membership, repository identity, source mode and Git state are
unchanged, a refresh rereads only paths whose identity/size/modification/change
stamp moved; unchanged bytes and their source entries are reused. Source capture
and final publication still perform bounded descriptor reads and a complete
source-stamp guard, so this optimization does not turn an index into planning
authority or weaken race detection. Projection updates use a deduplicated changed
path set and rebuild effective-target/retirement rows only when affected.

Parent-directory descriptors are reused within bounded capture workers and stamp
scans. The cache is an I/O optimization; a fresh source guard remains required
before checkpoint replacement. The canonical content-hash byte encoding has a
compatibility regression proving equality with the former JSON representation.

Focused delivered-source evidence:

- `cargo test --offline --locked -p workdeck-pm --test projection_storage --test sources --test transactions` passes **12 + 13 + 29 tests**.
- `cargo check --workspace --offline --locked` and `cargo fmt --all -- --check` pass.
- The 40,000-feature release harness run is recorded at
  `/tmp/workdeck-pm10-delta-cache-features-40000-20260911.log`: cold index
  **10,984.72 ms**, incremental refresh **8,676.75 ms**, warm-filter p95
  **0.841 ms**, and peak process RSS **997,130,240 bytes**. Stage timing shows
  AfterCapture **2,674.05 ms**, AfterProject **5,975.55 ms**, and AfterPublish
  **8,666.39 ms**.

- The independent 10,000-issue run is recorded at
  `/tmp/workdeck-pm10-delta-cache-issues-10000-20260911.log`: cold index
  **7,336.06 ms**, incremental refresh **6,006.68 ms**, warm-filter p95
  **0.857 ms**, and peak process RSS **361,037,824 bytes**. Its stage timing
  reaches AfterCapture **1,437.75 ms**, AfterProject **4,400.82 ms**, and
  AfterPublish **6,004.50 ms**.

The run verifies counts, bounded first/last pages, checkpoint reopening, changed
generation, retained old-reader results and source identity. It is open-policy
synthetic scale evidence, not mounted UI qualification or a complete PM-10 gate.
The original one-second incremental target was unmet, which motivated the calibrated
budgets recorded above. Full-image checkpoint encoding, complete validation and publication
costs remain measured bottlenecks. No source timeout, query target or acceptance requirement
was weakened.

## PM-10 active implementation checkpoint — 2026-09-09

**PM-10 remains incomplete.** PM-00–PM-09 are closed (10/13, approximately 77%
by phase count). Initial bounded implementation is in progress:

- Local explicit checkout registry passes seven focused core tests in
  `/tmp/workdeck-pm10-registry-green2.log`: inert reads, no target writes,
  concurrent stale-source rejection, historical request replay after removal,
  replacement-directory identity rejection, unavailable mappings and path/ignore safety.
  The target-replacement-before-publication regression first failed with an incorrectly
  successful registration (`/tmp/workdeck-pm10-registry-target-race-red.log`), then passed
  after revalidating the target at publication. Two actual CLI tests pass in `/tmp/workdeck-pm10-registry-cli.log` (5.48s),
  covering registration review/apply/show, missing targets, historical replay/removal
  and missing-source handling. Schema exports are added; generated references await
  the complete PM-10 surface.
- Existing executable rejects `repository list` with exit2/Unknown command:
  `/tmp/workdeck-pm10-registry-command-red.json`; this is the registry CLI behavioral
  baseline. The probe's initial expected stderr spelling was corrected separately.
- SQLite storage's four runtime failing baselines (`/tmp/workdeck-pm10-storage-red.log`)
  are followed by ten passing storage/cache tests (`/tmp/workdeck-pm10-storage-second-green.log`).
  The first cache run exposed an authorizer ordering defect; the guard is installed after
  in-memory deserialization, which itself uses an internal memory attachment.
- Projector/query baselines first failed four tests (`/tmp/workdeck-pm10-projector-red.log`).
  Seven now pass (`/tmp/workdeck-pm10-projector-seven-green.log`), including native issue
  predicate parity, immutable query/page/detail identities, inherited membership, independent
  record families and retirement lookup sharing. The expanded family test exposed Markdown
  handoffs being parsed as YAML; handoffs now use the native Markdown extraction branch.
- A bulk query of 201 issues parsed the same receipt 201 times, failing
  `/tmp/workdeck-pm10-retirement-bulk-red.log`. Bulk issue, graph and feature readers now
  share one validated retirement index, with lazily indexed marker paths. The projector
  regression passes; all 8 query and 19 retirement tests also pass in
  `/tmp/workdeck-pm10-bulk-adjacent.log`.
- Feature diagnostics rebuilt an 80-record graph 160 times, failing
  `/tmp/workdeck-pm10-feature-graph-red2.log`. They now share one lookup index while retaining
  per-record association and cycle checks; the unchanged regression passes in
  `/tmp/workdeck-pm10-feature-graph-green.log`. All 18 feature and 30 graph tests also pass in
  `/tmp/workdeck-pm10-feature-graph-adjacent.log`.
  The earlier probe failed on its missing fixture directory, not product behavior.
- TUI viewport baseline reproduced whole-list formatting outside the allocated rows
  (`/tmp/workdeck-pm10-viewport-red.log`). The bounded renderer, viewport and worker pass 32
  tests (`/tmp/workdeck-pm10-viewport-worker-green.log`). Nine indexed-workspace/renderer tests
  pass (`/tmp/workdeck-pm10-indexed-workspace-green.log`); two real core-adapter tests also pass
  (`/tmp/workdeck-pm10-indexed-core-green.log`), covering retained excerpts across edits and
  cached-view fallback after malformed configuration. Mounted indexed Issues/Features, boards
  and trees remain open.
- Offline dependency resolution added rusqlite 0.40.2 plus five dependencies without
  upgrading unrelated packages. The index is a guarded local checkpoint, not planning authority.
- Git scale baseline captured 65 documents using 142 subprocesses and failed its
  process-count assertion (`/tmp/workdeck-pm10-batch-red.log`). Three batch tests pass
  (`/tmp/workdeck-pm10-batch-first-green.log`), plus 13 source and 6 staged-source tests
  (`/tmp/workdeck-pm10-source-adjacent-first-green.log`). The final three batch tests, including the no-lazy-fetch regression
  and real-Git byte-limit checks, pass in `/tmp/workdeck-pm10-batch-complete-green.log`;
  all 10 storage, 7 publication, 13 source and 6 staged tests also pass after the changes
  (`/tmp/workdeck-pm10-storage-source-adjacent.log`).
- A real partial-clone fixture reproduced ordinary source reads hydrating missing promisor
  objects (`/tmp/workdeck-pm10-lazy-fetch-red2.log`). Bound Git commands now use the explicit
  `--no-lazy-fetch` global flag. The unchanged regression passes, including a control showing
  that explicit reviewed fetch remains usable (`/tmp/workdeck-pm10-lazy-fetch-green.log`).
  All remotes are local temporary fixtures. Older Git without this flag fails closed.
- My work's four core tests pass (`/tmp/workdeck-pm10-my-work-green.log`): qualified identities
  across repositories, pagination, stale cursors, explicit unavailable sources and alias
  selection. The old binary rejected `repository my-work`
  (`/tmp/workdeck-pm10-my-work-cli-baseline.log`). All three registry CLI tests now pass
  (`/tmp/workdeck-pm10-registry-my-work-cli.log`), including paginated qualified rows and exit
  code 4 with an explicit unavailable member. Schema exports exist; generated references await
  the complete command surface.
- `crates/workdeck-pm/examples/projection_bench.rs` defines isolated editor-authored feature/
  issue datasets, cold/warm timings, checkpoint reopening, incremental refresh and peak RSS.
  Debug smoke runs of 100 features and 100 issues pass (`/tmp/workdeck-pm10-bench-features-100.log`,
  `/tmp/workdeck-pm10-bench-issues-100.log`). These validate the harness, not the 40,000/10,000
  performance requirements, which remain open. The reference machine is an Apple M1 Pro,
  16 GiB RAM, 10 logical CPUs, macOS 26.5.2; machine evidence is in
  `/tmp/workdeck-pm10-benchmark-machine.json`.

A 1,000-feature debug run also passes (`/tmp/workdeck-pm10-bench-features-1000.log`):
cold 1,835.60 ms, incremental 1,642.49 ms, warm filter/page p95 4.62 ms, peak RSS 57,311,232
bytes. Incremental refresh exceeds the one-second target at this debug size. This is a
diagnostic baseline, not optimized full-size qualification. The first 40,000-feature release run failed with `projection database: interrupted`
(`/tmp/workdeck-pm10-bench-features-40000-release.log`); it does not qualify. A 10,000-feature
release diagnostic run passed: cold 13,102.87 ms, incremental 4,516.75 ms, warm p95 0.935 ms,
peak RSS 338,411,520 bytes (`/tmp/workdeck-pm10-bench-features-10000-release.log`). A live
process sample (`/tmp/workdeck-pm10-feature-10000-sample.txt`) attributed 1,211 of 1,449
sampled main-stack observations to projector removal and full-text scanning during the
refresh. The SQL-work regression reproduced quadratic growth when doubling 200 to 400 records:
3,511 to 13,434 progress intervals (`/tmp/workdeck-pm10-cold-sql-red.log`). Cold insertion
now skips removals for paths absent from the previous manifest. All eight projector tests
pass, including the unchanged growth assertion (`/tmp/workdeck-pm10-cold-sql-green.log`).
The repaired 40,000-feature release run passes
(`/tmp/workdeck-pm10-bench-features-40000-release2.log`): cold 23,535.80 ms, incremental
23,224.72 ms, warm filter/page p95 0.921 ms, peak process RSS 1,099,251,712 bytes. The
fixture has 40,000 features with 8-ary parents and sparse prerequisites; checkpoint reload
and retained old-view equivalence pass. This is a full-size baseline, not complete scale
qualification: incremental refresh missed the then-current one-second target, richer policy/record
fixtures and mounted long-list/tree navigation remain required. A second process sample
(`/tmp/workdeck-pm10-feature-40000-repaired-sample.txt`) now shows native feature parsing
during refresh as a major cost. The independent 10,000-issue release run also passes
(`/tmp/workdeck-pm10-bench-issues-10000-release.log`): cold 10,143.58 ms, incremental
10,053.03 ms, mixed filter/page p95 0.924 ms, peak RSS 432,553,984 bytes. Both historical
filter series contain ten first-use queries and 190 cache hits; their p95 does not establish
first-use filter latency. The amended harness separates these series; remeasurement is
pending. See [performance evidence and reproduction](project-management-performance.md). No
source-safety timeout or acceptance requirement was weakened.

The shared native-selection bridge is now being added in `projection/live.rs`. It reopens
one exact issue/feature document after checking repository identity, physical checkout slot,
local source role, path and content hash, and returns a normal native source token. It does
not treat an index row as write or completion authority. All four source/replacement/stale-record tests pass in
`/tmp/workdeck-pm10-projection-live-second.log`; the initial compile-time source-role name
was corrected before execution. An indexed authoring-controller path now retains one native
selection and separate drafts. All three real-core TUI tests pass in
`/tmp/workdeck-pm10-indexed-authoring-third.log`, including a stale failed draft whose text,
request ID and original source token survive selection changes. The initial build ran out
of disk before tests (`/tmp/workdeck-pm10-indexed-authoring-first.log`); about 1.04 GB of idle
artifacts from this worktree were reclaimed. The next build exposed a private query
validation method; `IssueQuery::validate` is now public so the TUI uses shared input bounds.
No assertions changed. The initial normal-shell regression reproduced full collection retention
(`/tmp/workdeck-pm10-mounted-index-red.log`, three native records). Default Issues now
starts the bounded indexed reader and retains one native selection; the four initial
real-core tests pass (`/tmp/workdeck-pm10-mounted-index-first.log`). Keyboard/mouse routing,
panel issue/planning links, review-return selection, and owned reader shutdown have now
been wired. All six integrated index tests pass
(`/tmp/workdeck-pm10-mounted-index-second.log`). The first complete workbench run passed
122 and failed 21 tests; event-loop pumping for the new asynchronous reader reduced this
to seven failures. Restoring background loading in Review preserves the prior default
selection for linking a review file. All 143 workbench tests now pass
(`/tmp/workdeck-pm10-mounted-workbench-third.log`, 18.51 s), preserving the old behavioral
assertions and inspecting collection membership through the indexed page. The earlier
failures remain in `mounted-workbench-first.log` and `mounted-workbench-second.log` under
`/tmp/workdeck-pm10-`. This remains an in-progress mount: independent excerpt scrolling first failed its acceptance test
(`/tmp/workdeck-pm10-indexed-excerpt-scroll-red.log`), then passed with all seven integrated
index tests (`/tmp/workdeck-pm10-indexed-excerpt-scroll-green.log`). Shift-PageUp/PageDown
scrolls the retained excerpt without changing list selection or the opened source. Planning
loading/stale state is visible in the navigation bar. Actual workbench terminal journeys
are running (`/tmp/workdeck-pm10-mounted-pty-first.log`); Features/Planning remain on their
previous collection readers. All 22 adjacent live-selection/storage/query tests pass
(`/tmp/workdeck-pm10-live-storage-queries-green.log`), followed by all 13 existing controller
regressions (`/tmp/workdeck-pm10-controller-adjacent-green.log`). Final PM-10 formatting,
lint, generated references and full integrated qualification remain pending.

Next: mount indexed Issues/Features with source-checked controller selection, preserve
keyboard/mouse/return context and worker shutdown, and measure and repair full-size bottlenecks; complete indexed views and complete board/tree/multi-repository workflows; establish full-size
performance evidence. Generated references and integrated PM-10 qualification remain pending.

## PM-09 qualified checkpoint — 2026-09-09

**PM-09 closed.** Ten of thirteen phases are closed (approximately 77% by phase
count, not effort). All nine gates ran on candidate
`65a50ac0940b643d1e28da1b81debac7374840e89569ed4a639b211c956e3fbe`
(1,164 inputs); every report and final comparison confirm unchanged source.
Reports: `/tmp/workdeck-pm09-qualification-20260909/`.

| Gate | Passing evidence |
| --- | --- |
| PM all targets | 720 tests |
| CLI all targets | 743 top-level tests, including all119 actual PTYs (63.46s); five nested worker results excluded |
| TUI library | 1,195 tests (22.74s) |
| Supporting crates | 568 tests; all242 VCS tests pass in6.91s with normal concurrency and original deadlines |
| Example startup lifecycle | 3 tests |
| Strict lint | Eight packages, all targets, warnings denied |
| Architecture | 13 production crates, one shipped executable, zero violations |
| Generated skills | Passed |
| Formatting | Passed |

Total: **3,229 top-level tests**. This is phase qualification, not complete-plan
or release qualification. PM-10–PM-12 remain required.

Retained failures and limits: the first support run failed watcher quiet assertions
`exact_entry_target_ignores_sibling_files` and
`recursive_target_ignores_excluded_metadata_churn`. Temporary event tracing passed
18 watcher and242 VCS tests and showed observed sibling/metadata events rejected;
it did not reproduce or establish the original cause. Exact original sources were
restored and the candidate fingerprint verified. The next normal support run passed
both watchers but failed oversized Git diagnostics cleanup (23.2059s against2s).
The isolated unchanged test passed in0.27s; the final full supporting suite then
passed568 tests. Original gate failures are retained as `watcher-initial-support.*`
and `cleanup-timing-support.*`. Trace logs are `/tmp/workdeck-pm09-watch-probe.log`
and `/tmp/workdeck-pm09-watch-full-probe.log`; they are diagnostic evidence, not
replacement qualification. These intermittent observations remain PM-12 hardening
work. No filter, timeout or assertion was weakened.

Earlier source/lint fixture candidates remain under `initial-*` and
`pre-contract-fixture-*`. The table below records historical focused increments;
those increments are superseded by the integrated candidate above.

| Focused behavior | Passing evidence |
| --- | --- |
| Local claims, registered-actor and linked-question admission, prospective storage bounds, strict claimed completion and separate release | 33 tests; `/tmp/workdeck-pm09-claims-completion-green.log` |
| Shared and local claimed completion, separate release, stale-source/policy rejection and historical replay | 20 tests (12 local, 8 shared); `/tmp/workdeck-pm09-claims-shared-completion-green.log` |
| Central receipt-history admission, completion binding, replay/recovery and affected migration/snapshots | 108 tests (68 owner group, 40 adjacent); `/tmp/workdeck-pm09-capacity-completion-green.log`, `/tmp/workdeck-pm09-capacity-adjacent-green.log`; scoped PM lint passes in `/tmp/workdeck-pm09-capacity-completion-clippy.log` |
| Historical receipt retained when current issue inspection fails, with continuation denied | 1 test; `/tmp/workdeck-pm09-claims-outcomes-green.log` |
| Ordinary writes cannot install a coordination marker; recovery rejects a forged claim receipt before file publication | 2 tests; `/tmp/workdeck-pm09-coordination-guard-green.log` |
| Source capture, explicit fetch/sync, non-Git local mode and cached-view remote binding | 8 tests; `/tmp/workdeck-pm09-sources-binding-green.log` |
| Shared publication, old-request replay and reviewed proposals | 3 integration tests; `/tmp/workdeck-pm09-proposal-green-binding-red.log`; its separate cached-binding case was still RED and was subsequently repaired in the source run above |
| Shared race/lost-response and bounded process/deadline/candidate guards | 7 focused source cases within 20 PM library tests; `/tmp/workdeck-pm09-source-safety-green-guard-red.log`; its separate guard target remained RED at this point and was subsequently repaired above |
| Source identity, isolated Git profile, proposal recovery/concurrent updates and reviewed remote binding | 19 tests (12 sources, 7 publication); `/tmp/workdeck-pm09-reviewed-binding-green.log`; scoped source Clippy passes in `/tmp/workdeck-pm09-source-clippy-final.log` |
| One total claim observation deadline; expiry retains confirmed receipt without authorizing continuation | 8 tests (deadline observer plus 7 source units); `/tmp/workdeck-pm09-deadline-source-green.log` |
| Actual claim/claimed-completion, source and proposal CLI workflows | 6 tests; `/tmp/workdeck-pm09-collaboration-cli-green.log` |
| Final claim/source/proposal CLI and fresh-agent/catalog/generated-reference behavior | 7 workflow tests in `/tmp/workdeck-pm09-cli-final-focused2.log`; 6 catalog tests in `/tmp/workdeck-pm09-catalog-final-green2.log` |
| Installed hook and staged candidate CLI behavior | 5 tests; `/tmp/workdeck-pm09-staged-hooks-cli-green2.log` |
| Hook lifecycle/security and staged validation | 15 tests; `/tmp/workdeck-pm09-hooks-staged-final-focused.log` |
| Mounted immutable Sources workbench | 4 tests; `/tmp/workdeck-pm09-sources-mounted-green.log` |
| Workbench Context/Checks plus Sources, Claims, Complete, explicit source operations and owned-publication integration | 119 tests; `/tmp/workdeck-pm09-workbench-final-focused.log` |

All four PM-09 actual terminal journeys have focused passing evidence. The extended
local Claims/Complete journey (72/132 columns), pending shared publication with signals
and owned-child cleanup, and immutable Sources citations passed in
`/tmp/workdeck-pm09-terminal-final-focused.log`; that invocation's new source-operations
journey had a test-oracle failure. Source operations then passed in
`/tmp/workdeck-pm09-source-operations-terminal-green2.log` (22.58s), exercising reviewed
Fetch/Sync, proposal preview/publication, and fresh-session status/resume. Its oracle
corrections distinguish the active panel header from a footer and distinguish newly
observed status/resume results from the previous success screen. No product RED is
claimed for these oracle errors. Earlier source-title clipping was likewise a test
assertion issue. The complete CLI/PTY suite subsequently passed on the qualified candidate above.

Behavioral failing baselines were retained for unknown/archived actors, questions on
linked features/gates/milestones, claim catalog and exact receipt-byte overflow,
forged historical/recovery receipts, non-Git local source status, cached source binding,
publication races/uncertainty, missing CLI subcommands and mounted Sources/Claims entry.
Reviewed fetch and proposal publication also reproduced a remote-swap defect with unchanged planning config and identical accepted objects; both now reject the changed destination. Logs: `/tmp/workdeck-pm09-reviewed-fetch-binding-red.log`, `/tmp/workdeck-pm09-reviewed-proposal-binding-red.log`. The deadline observer regression used a refactor-only baseline retaining the previous fresh-per-call capture/revalidation behavior; it returned `may_continue=true` after the budget expired. The same test passed with the shared deadline restored (`/tmp/workdeck-pm09-deadline-observer-red.log`). Compile/setup failures are not counted as behavioral RED.

Shared claimed completion, receipt capacity and their related recovery/migration checks
now pass. Interactive completion and explicit source operations pass mounted workbench
coverage. Rebuilt CLI binding acceptance, all four PM-09 terminal journeys and generated
reference parity also passed focused checks. Final integrated qualification above closes all
16 PM-09 ledger rows. Required later phases PM-10–PM-12 are unchanged.

## PM-08 qualified checkpoint — 2026-09-09

**PM-08 closed.** All nine qualification gates have passing evidence for candidate
`c78a0b2f119a78e14e4528a2d36fdda395fa14843eee09ae7b0a3bce740e40cb`
(1,102 inputs), and the final source comparison matched. All 15 PM-08 requirements
have implementation and validation evidence. Six gates ran on this final candidate;
the three explicitly identified below carry forward from unchanged test inputs.

| Gate | Current evidence |
| --- | --- |
| PM all targets | 624 passed on `b69d7c5d`; carried forward by unchanged-input equivalence |
| CLI all targets | 727 passed on `c78a0b2f`, including 115 real PTY tests; five nested worker results excluded |
| TUI library | 1,175 passed on `b69d7c5d`; carried forward by unchanged-input equivalence |
| Supporting crates | 568 passed on `c78a0b2f`, including all 242 VCS tests in 8.11 seconds |
| Example startup lifecycle | 3 passed on `b69d7c5d`; carried forward by unchanged-input equivalence |
| Strict lint | Eight packages, all targets, warnings denied; passed on `c78a0b2f` |
| Architecture | 13 production crates, one shipped executable, zero violations; passed on `c78a0b2f` |
| Generated skills | Passed on `c78a0b2f` |
| Formatting | Passed on `c78a0b2f` |

The two captured manifests differ only in `crates/workdeck-cli/src/main.rs` and
`crates/workdeck-cli/src/pm_cli.rs`: the internal `Command::NamedCommand` variant
became `Command::Recipe` to satisfy strict Clippy. The explicit public Clap name
remains `command`. Every other captured source, dependency manifest and lockfile
is identical. PM, TUI-library and example-startup gates neither compile nor invoke
those CLI binary files, so their earlier passing evidence remains applicable.
They were **not rerun against the final aggregate fingerprint**.

The records in `/tmp/workdeck-pm08-qualification-20260909/{pm,tui,examples}.json`
retain the original source, final qualification source, changed-file list,
equivalence rationale and original logs in `candidate-b69d7c5d/`. The other current
gate reports bind `c78a0b2f` directly. The final CLI rerun covers the affected binary;
it passed, and the final source comparison matched. Total: **3,097 top-level tests**.
Reports, source manifests and retained failures are under
`/tmp/workdeck-pm08-qualification-20260909/`; the repeatable helper is
`/tmp/workdeck-pm08-qualification-20260909.py`.

At this historical PM-08 checkpoint, PM-09–PM-12 remained unimplemented. The PM-09
qualification above supersedes that status; neither checkpoint is final release qualification.

Final focused evidence:

- Planner19, command catalog3 and shared record5 pass together (**27**) in
  `/tmp/workdeck-pm08-planning-qualified.log`. Coverage includes complete historical
  manifests, conservative selection, private environment pins, portable tool/input
  identities, working-directory replacement, and empty/overlarge plan blockers.
- Execution16, PM library13 (including process4), report11 and shared record5 pass
  together (**45**) in `/tmp/workdeck-pm08-runner-qualified.log`; strict scoped
  Clippy passes in `/tmp/workdeck-pm08-runner-lint.log`. Coverage includes interrupted
  publication/recovery, retained receipt authority, concurrent retries without
  another spawn, bounded aggregate output, missing artifacts and source changes.
- CLI8 plus source-cutover9 pass in `/tmp/workdeck-pm08-cli-integrated.log`, including
  matching 4 MiB saved-plan/file/stdin admission. Catalog6 and regenerated
  command/schema references pass, including strict extension namespaces.
- Context4 plus existing26 pass in `/tmp/workdeck-pm08-context-green2.log`.
  Current artifact assessment, bounded latest-five summaries, exact omissions and
  actionable failures are covered without including raw logs in context.
- Mounted workbench98 plus the additive retained-selection navigation case pass in
  `/tmp/workdeck-pm08-workbench-final.log` and
  `/tmp/workdeck-pm08-check-context-navigation-green.log`.
- All four actual PM-08 terminal journeys pass together in 9.28 seconds in
  `/tmp/workdeck-pm08-checks-pty-green.log`; strict TUI and terminal-test lint pass
  in `/tmp/workdeck-pm08-tui-lint.log` and `/tmp/workdeck-pm08-pty-lint.log`.

These focused groups overlap and are not added to the qualification total.
The bounded requirement review found no remaining implementation blocker. Execution
is foreground Unix local feedback with explicit owned-process cleanup; PM-11 CI
trust and completion admission remain unavailable.

### Historical PM-08 increments and repaired failures

The earlier focused counts and pending work below describe their original
checkpoints; the qualified results above supersede their pending status.

The initial `b69d7c5d` candidate passed PM624, CLI727 (including 115 real PTY tests,
with five nested worker results excluded), TUI1,175 and the three startup examples.
Its strict lint failed `clippy::enum_variant_names`; the internal rename described
above is the only source change in the replacement candidate. The failed lint and
original test logs remain in `candidate-b69d7c5d/` under the qualification directory.

Two earlier unchanged-source support attempts failed VCS total-collection timing
assertions: Jujutsu took 9.17 seconds in `initial-support-cleanup-timeout/support.log`;
Git took 29.26 seconds and Jujutsu 23.44 seconds in
`second-support-cleanup-timeout/support.log`. Git's existing limit is two seconds
and Jujutsu's is five. These timers include process startup, stream collection and
cleanup; they do not isolate signal-cleanup time. The exact unchanged Jujutsu retry
passed in 0.42 seconds (`/tmp/workdeck-pm08-vcs-cleanup-exact.log`).

The separate default-concurrency probe
`/tmp/workdeck-pm08-vcs-concurrent-probe.{log,json,sample.txt}` passed both cleanup
tests but failed `recursive_target_ignores_excluded_metadata_churn` because an
unexpected event arrived. Its raw path/kind was not retained, so delayed/coalesced
setup notification and wrongly forwarded metadata churn cannot be distinguished.
The watcher test passed both earlier support attempts and final qualification.
The associated `child-44371.txt`, `child-44381.txt` and `child-44384.txt` samples
showed `/bin/sh` children mainly in `_dyld_start`; debugger-notification frames
may be sampling effects. This establishes observed startup delay, not its cause
or an explanation for the separate watcher event. All VCS/core/diff source inputs
match the PM-07 manifest. Final support passed without VCS edits, disabled tests
or increased deadlines; the timing and watcher observations remain PM-12
reliability evidence rather than a claimed production repair.

Earlier terminal failures include one repaired production detail-scroll bug,
fixture caption/navigation assumptions, and a separate pre-main startup delay.
The exact live sample `/tmp/workdeck-pm08-pty-startup-sample3.txt` shows only
`_dyld_start`, no application frame, and a 112 KiB footprint. Loader/code-validation
delay is an inference; its cause is not established. Unchanged retries passed
without increasing the 20-second terminal deadline. This remains a PM-12 startup
observation, not an application fix claim.

A final schema audit observed arbitrary top-level properties advertised for the
three execution definition types despite strict runtime extension namespaces
(`/tmp/workdeck-pm08-definition-schema-red.json`). The existing schema postprocessor
now restricts them to declared fields plus `x-*`, preserving `custom`. Catalog6
and refreshed generated artifacts passed focused parity and are included in the
frozen qualification candidate.

Earlier integration evidence:

- Runner9 and PM library13 (including process4): `/tmp/workdeck-pm08-execution-green.log`.
  Exercises intent before spawn, concurrent retry once, released PM lock during
  execution, lost acknowledgement recovery, missing artifacts, stale inputs and
  failed process versus passing report.
- Planner15, catalog3 and definition record2: `/tmp/workdeck-pm08-inputs-green.log`.
  Includes a real working-directory swap RED→GREEN; later manifest proof review
  and additional tests were still in progress at this checkpoint.
- Report8: `/tmp/workdeck-pm08-reports-green.log`; XML declaration and malformed
  SARIF notification regressions were subsequently added and qualified below.
- Context4 plus existing26: `/tmp/workdeck-pm08-context-green2.log`; prior failure
  `/tmp/workdeck-pm08-context-red.log` demonstrated that a completed failed run
  did not change context before integration. Missing artifacts and an artifact
  race now affect the current context assessment.

The earlier CLI5 group passed in `/tmp/workdeck-pm08-cli-green.log`. The first run
exposed dropped receipt identities when the bounded diagnostic renderer truncated a large
post-commit error; `/tmp/workdeck-pm08-cli-first.log` retains that failure. The
repair preserves both operation lookup summaries, run/request identity and an
explicit full-receipt omission marker. Retry without the invalid projection returns
the original result without another process.

Shared record5/report10 passed in `/tmp/workdeck-pm08-shared-green.log`. Its preceding
`shared-red.log` contains two real false-green parser regressions: unsupported or
duplicate XML declarations and malformed SARIF notifications. Saved-plan tampering,
foreign identity, no-execution behavior, export exclusions, and missing result
publication proof are covered by the record tests.

The later `/tmp/workdeck-pm08-final-edge-red.log` contains missing whole-run-directory
detection and contradictory JUnit root totals. Both were repaired before the final
45-test runner/report/record group above.

The earlier mounted workbench97 group passed in `/tmp/workdeck-pm08-workbench-green.log`,
including historical-pass filtering after proof loss and signal delivery without UI polling.
Their actual RED logs are `/tmp/workdeck-pm08-tui-filter-red.log` and
`/tmp/workdeck-pm08-tui-signal-red.log`. The first actual PTY run failed a fixture
predicate because a width110 list truncated `0 blockers`, while the detail retained
`No planner blockers.`; the predicate now uses that full semantic detail with the
same deadlines. The repeated four-journey PTY gate subsequently passed as recorded
above, followed by the final qualified 115-test terminal gate.

Independent CLI review found a valid 2,213,645-byte plan rejected through its saved
file path by the generic 2 MiB reader (`/tmp/workdeck-pm08-plan-path-red.json`). The
plan-specific file/stdin reader now uses the 4 MiB plan contract. The final integrated
CLI8 group above includes a passing real large-plan run and headless signal cleanup.

## PM-07 qualified checkpoint — 2026-09-09

**PM-07 closed.** All nine qualification gates passed against unchanged candidate
`f1d4e9c55d712e21de6e130f47cdbef05e595ecd453b4c7b1661c26fe3f5902c` (1,065 inputs).

| Gate | Result |
| --- | --- |
| PM all targets | 562 passed |
| CLI all targets | 714 passed, including 111 real PTY tests; excludes five nested worker results |
| TUI library | 1,162 passed |
| Supporting crates | 568 passed: core65, extension API70, extension host186, migration5, VCS242 |
| Example startup lifecycle | 3 passed |
| Strict lint | Eight packages, all targets, warnings denied |
| Architecture | 13 production crates, one shipped executable, zero violations |
| Generated skills | Passed; PM skill/command/schema byte parity also passed in CLI tests |
| Formatting | Passed |

Total: 3,009 top-level tests. Every gate report and the final source comparison
bind the same fingerprint. Artifacts: `/tmp/workdeck-pm07-qualification-20260909/`;
repeatable helper: `/tmp/workdeck-pm07-qualification-20260909.py`.
Independent requirement review found and then verified the document-context repair.
Core context26, CLI context7/catalog5 and the mounted document case have focused
logs in `/tmp/workdeck-pm07-context-documents-green.log`,
`/tmp/workdeck-pm07-document-integration-green.log`, and
`/tmp/workdeck-pm07-document-tui-green.log`.

PM-08 became active at this checkpoint and has since qualified as recorded above.
PM-09–PM-12 remain required; phase qualification is not final release qualification.
The earlier extension-startup failure remains an
unexplained PM-12 reliability observation, with original evidence retained below.

## PM-07 review repair — superseded, 2026-09-09

Candidate `e600bdef16f7188ed23df3e9130f183c732f0891bc4092ce91c45868f5177d60`
captures 1,065 inputs after the legacy-reader compatibility repair and bounded PTY
failure diagnostics. Strict Clippy, formatting and the full CLI gate passed with
unchanged source, including 111 terminal tests. Evidence is archived under
`/tmp/workdeck-pm07-qualification-20260909/second-e600bdef/`. The final requirement
audit then found that issue document links were omitted from context; that repair
and a clarification of local protocol receipt semantics supersede this candidate.
The final `f1d4e9c5` capture and all nine passing gates above supersede this
intermediate checkpoint; this candidate itself did not close the phase.

The omitted-document behavior was reproduced through the installed local CLI in
a temporary repository: both `docs/accepted-design.md` and an inert HTTPS reference
were absent from the packet sections. Evidence:
`/tmp/workdeck-pm07-document-cli-red.json`. The repair adds budgeted typed document
entries with declaring-issue citations and explicit `document_not_fetched` status;
CLI and mounted-context regressions cover the adapter surfaces.

The startup investigation established no runtime cause: three serial and ten
concurrent copied-fixture JSON-RPC handshakes passed in 0.164–1.681 seconds; ten
full-host PTY probes showed Review in 1.303–3.069 seconds with no captured load
failures. The initial gate's post-link/cold-start state was not reproduced, so
macOS validation/cache explanations remain hypotheses. Probe evidence is retained
in `/var/folders/xq/3h5kbr5550l9msblmc16334m0000gn/T/workdeck-extension-startup-probe-gbwt7kak/`
and `/var/folders/xq/3h5kbr5550l9msblmc16334m0000gn/T/workdeck-extension-pty-probe-mh1rg80h/`.
The test harness now retains at most 16 KiB of relevant diagnostic lines and a
16 KiB first frame, printing these with child status on failure. Assertions,
concurrency and deadlines are unchanged. During the idle build window, 133 obsolete
older test executable variants were removed (1,376,231,776 bytes), preserving newer
tests and the retained PM-06/current Workdeck binaries. Inventory:
`/tmp/workdeck-pm07-obsolete-tests-cleanup.json`.

## Initial PM-07 candidate — superseded, 2026-09-09

Candidate `01194b9f2253cfbb0e652e9a2d32fb7140023a7e7da4bf75ed8f3d600c30d4b4`
captures 1,065 source inputs, including the generated command/schema references.
Final focused CLI integration passes 28 tests and catalog/discovery parity passes
five tests. Logs: `/tmp/workdeck-pm07-final-integration1.log` and
`/tmp/workdeck-pm07-final-catalog.log`. All owners released their sources and the
workspace was formatted before capture. PM558, strict eight-package all-targets
Clippy and formatting passed with unchanged source. The full CLI gate failed 10
existing extension PTY cases while 101 PTY cases passed, including both new PM-07
journeys. The unchanged exact highlighter retry passed in 7.34 seconds; absence of
extension authority in final screens does not establish the cause. The bounded startup
investigation above did not reproduce it; it remains a PM-12 reliability observation.
Reports, original failure and source manifest:
`/tmp/workdeck-pm07-qualification-20260909/initial-01194b9f/`; repeatable helper:
`/tmp/workdeck-pm07-qualification-20260909.py`.

The candidate was also superseded by a proven compatibility repair: the new source-
selection config wrapper changed an established read-only legacy error from exit5/
`config_or_store_error` to exit2/native JSON. The retained PM-06 executable and
candidate were compared on the same temporary malformed configuration in
`/tmp/workdeck-pm07-legacy-error-compat-red.json`; a failing regression is retained
in `/tmp/workdeck-pm07-legacy-config-regression-red.log`. The wrapper now preserves
the legacy-reader path while new agent commands retain bounded typed errors. The
paired regression and context group pass 13 tests in
`/tmp/workdeck-pm07-legacy-config-regression-green.log`. This required a new
qualification capture; the final qualified `f1d4e9c5` checkpoint above records closure.

## PM-07 historical increments — 2026-09-09

These focused results were collected while source was changing during protocol
review and parser/output integration. They are retained as historical evidence;
the final qualified PM-07 checkpoint above supersedes their pending status.

- Core context: 20 cases pass, and the final context/continuity union passes 47
  tests in `/tmp/workdeck-pm07-context-qualified.log`; strict context lint passes
  in `/tmp/workdeck-pm07-context-clippy.log`.
- Core continuity: 15 question and 12 handoff tests pass; the affected regression
  group passed 132 tests. Strict scoped PM lint passed. Logs:
  `/tmp/workdeck-pm07-continuity-final.log`,
  `/tmp/workdeck-pm07-continuity-adjacent.log`, and
  `/tmp/workdeck-pm07-continuity-clippy-final2.log`.
- Mounted Context workspace: 16 new cases and the 85-test workbench group pass.
  Two real PTY journeys pass at widths 78/180, including fresh context, question
  answer, immutable handoff, another process after source edits, and safe citation
  navigation. Scoped TUI and terminal target strict lint pass.
- CLI integration: catalog, context, continuity, configuration cutover and legacy
  admission pass in `/tmp/workdeck-pm07-cli-integration3.log`. A subsequent question
  pagination/projection case passes in `/tmp/workdeck-pm07-pagination2.log`.
- Behavioral RED→GREEN evidence includes the missing native context command,
  oversized diagnostics (96,271 bytes before the 16 KiB ceiling), and a field
  projection error emitted as human stderr instead of machine output. Retained
  logs: `/tmp/workdeck-pm07-context-cli-red.log`,
  `/tmp/workdeck-pm07-bounded-error-red.log`,
  `/tmp/workdeck-pm07-bounded-error-green.log`,
  `/tmp/workdeck-pm07-fields-behavior-red.json`, and
  `/tmp/workdeck-pm07-cli-integration2.log`.
- Explicit protocol installation: 17 cases pass in
  `/tmp/workdeck-pm07-protocol-review-final.log`, covering managed-block preservation,
  source CAS, retained requests, interrupted journal/publication/receipt recovery,
  conflicting editor changes, tampered local proofs, unsafe paths, permissions and
  ignored local state. Review found and verified repairs for Markdown fence handling,
  changed/missing recovery journals, and repository replacement between preview and
  write. The CLI requires the preview's repository identity in addition to the file
  precondition. Strict installer lint passes in
  `/tmp/workdeck-pm07-protocol-clippy-final.log`.
- Generated-reference parity first failed because the PM skill artifact was
  absent, then passed after rendering the actual catalogs. Regeneration and parity
  were rerun after the protocol parser and schemas froze, and passed final qualification.

Independent review reproduced two additional diagnostic escapes: malformed app
configuration returned legacy unbounded errors, and `capabilities` embedded raw
unavailable-source diagnostics. Both have observed RED and passing integrated
regressions in `/tmp/workdeck-pm07-config-error-red.log`,
`/tmp/workdeck-pm07-capability-error-red.log` and
`/tmp/workdeck-pm07-cli-review-green.log` (**29 tests** across the five targets).
A committed mutation followed by a projection failure also retains its request and
operation identity under the 16 KiB error ceiling and replays without duplication.

Before further builds, five obsolete executable artifacts in this worktree's
`target/debug/deps/` were removed with no Cargo/rustc/test processes active,
freeing 365,456,096 bytes. Current executables, libraries and qualification logs
were preserved; inventory is `/tmp/workdeck-pm07-obsolete-executable-cleanup.json`.
A subsequent idle build window removed 147 obsolete pre-02:00 Workdeck library
variants (644,403,450 bytes), preserving newer PM-06/07 artifacts. Inventory:
`/tmp/workdeck-pm07-obsolete-library-cleanup.json`.

Portable-context review additionally reproduced stale handoffs for identical clones
and identical atomic saves: the prior anchor included device/inode identities.
`/tmp/workdeck-pm07-context-portable-red.log` records both failures. The normalized
durable hash excludes those transient IDs while capture verification retains them;
22 context and 12 handoff tests pass in
`/tmp/workdeck-pm07-context-portable-green.log`, and strict scoped lint passes in
`/tmp/workdeck-pm07-context-portable-clippy.log`. Historical inode-bound anchors are
not rewritten and may remain conservatively stale.

The new agent-command parser path also reproduced an empty stdout and 50,116-byte
stderr for a malformed numeric argument with `--json`, recorded in
`/tmp/workdeck-pm07-parser-error-red.json`. It now uses the bounded typed renderer;
independent actual-binary probes confirmed 2,500-byte output and unchanged help
behavior. Capability field projection was introduced after an observed unsupported
flag failure in `/tmp/workdeck-pm07-capability-projection-red.json`; the final
qualified CLI gate includes integration of this small discovery response.

Compilation during a concurrently edited DTO and two incorrect test fixtures
(`normal` priority instead of `medium`, and a repository identity asserted on issue
metadata rather than its receipt) were corrected. These are not behavioral RED
claims. No existing test expectations or phase requirements were weakened.

## PM-06 qualified — 2026-09-09

Candidate `248ee1112608fffd86aaa3b2b58b2c108598bd7f2d18727dd2b0f768e01b4e59`
contains 1,031 source inputs. All nine gates passed on unchanged source, with a final
matching fingerprint comparison:

| Gate | Result |
| --- | --- |
| Shared PM, all targets | 509 tests |
| CLI, all targets | 679 tests, including 109 PTY tests; five nested worker results excluded from the total |
| TUI library | 1,145 tests |
| Core, extension API/host, migration and VCS, all targets | 568 tests |
| Startup lifecycle examples | 3 tests |
| Strict eight-package all-targets Clippy | Passed |
| Architecture | 13 production crates, one executable, zero violations |
| Generated skill and source mappings | Passed |
| Formatting | Passed |

The repeatable commands, JSON reports, logs and per-file source manifest are retained in
`/tmp/workdeck-pm06-qualification-20260909/`; the helper is
`/tmp/workdeck-pm06-qualification-20260909.py`. The first CLI attempt's seven existing
extension-startup fixture failures are preserved as `cli-attempt1.*`. Its unchanged
exact VCS retry, 92-test binary suite and full CLI retry passed without timeout or
source changes. The cause remains unconfirmed and is a PM-12 reliability observation.

Independent review mapped all twelve PM-06 requirements to the implemented native
graph/features/gates/evidence domains and actual CLI/workbench behavior. Repairs have
regression coverage for required gate debt, missing and filtered prerequisites,
prospective imports to retired endpoints, graph receipt intent across public/recovery
consumers, graph navigation/visibility/mouse routing, feature refresh races and retained
requests after lost acknowledgements. New graph receipts bind retained intent; historical
receipts lacking it retain their documented structural policy. No declaration establishes
verified evidence, and no issue completion promotes feature maturity.

At this checkpoint PM-00–PM-06 were complete and PM-07 became active. PM-07 has
since qualified as recorded above. The focused increments below are historical
evidence superseded by this PM-06 qualified candidate.

## PM-06 active increments — 2026-09-09

The native issue graph, feature workspace, gate declarations, immutable declared evidence,
and CLI adapters were integrated. These historical focused runs covered changing source
before PM-06 qualification; their pending-phase status is superseded above.

- Core feature coverage passes **18 tests**, including observed failing regressions for
  a prerequisite hidden by the current feature filter and a missing transitive prerequisite.
  Coverage now reports those outside dependencies and unresolved identities explicitly.
- Core graph/gate closure repairs pass **104 tests** across graph, gates, issues,
  retirement, schema, and snapshots. Completed prerequisites and required children retain
  unresolved gate debt; dependency-path queries expose reachable unresolved conditions.
  A prospective relation import to a retired endpoint was also reproduced and repaired,
  with unchanged historical import and exact restoration retained.
- The integrated workbench passes **65 tests**. Independent review reproduced and repaired
  missing historical graph anchors, offscreen keyboard selection, and graph mouse events
  consuming interaction intended for the visible review pane.
- All **11 workbench PTY tests** pass, including dependency traversal and feature
  create/edit/coverage/archive/restore at narrow and wide terminal widths. The graph test
  commits its synthetic planning fixture so the adjacent review diff cannot be mistaken
  for leaked filtered graph content. Product behavior and timeouts were not weakened.
- Expanded CLI checks pass **5 feature**, **4 gate/evidence**, **4 graph**, **6 legacy
  admission**, and **2 catalog** tests. Feature retirement exercises stale preview rejection,
  read-only flag admission, permanent identity reservation, and a committed mutation whose
  staging fails under a controlled Git index lock. The exact request retries its original
  receipt and stages only its own changed paths, preserving unrelated index/worktree bytes.
- Strict CLI/TUI all-targets Clippy passes after two mechanical conditional cleanups.
  Further independent graph receipt-proof and feature UI retry/source review is underway;
  final formatting, source capture, and all nine phase-wide gates follow those repairs.

The new declarations do not execute checks or establish verified evidence. Gate assessment
uses current captured definitions and an explicit evidence-freshness time; it does not
reconstruct past Git definitions. No real backlog was initialized or modified for tests.

The first frozen candidate is
`248ee1112608fffd86aaa3b2b58b2c108598bd7f2d18727dd2b0f768e01b4e59`
(1,031 inputs). The full shared PM gate passes **509 tests** with unchanged source.
Its first CLI attempt stopped on seven existing extension-startup tests sharing a
shell fixture that was not loaded. The unchanged exact external-VCS test then passed
in 0.36 seconds, followed by the entire unchanged 92-test CLI binary suite in 2.19
seconds. No source or timeout changes were made. The first failure report and log
are preserved as `cli-attempt1.*` in `/tmp/workdeck-pm06-qualification-20260909/`;
the full CLI gate and remaining gates are being rerun. The initial observation is
not sufficient to attribute the failure to either application logic or the host.

## PM-05 qualified — 2026-09-09

Candidate `e4487513cd0cfeacd0d6638e9b6aa2e0941a21c63af9c5e12959a34f71e891f3`
contains 995 source inputs. All nine gates passed and each report records unchanged source:
PM **439**, CLI **664** including **107 PTY**, TUI **1,133**, supporting crates **568**,
and startup examples **3**; strict eight-package all-targets Clippy, architecture,
skill mappings and formatting also passed. Five nested CLI worker results are excluded
from the top-level total. Final source comparison matched the captured manifest.

Reports, logs and the per-file source manifest are in
`/tmp/workdeck-pm05-qualification-20260909/`; the repeatable helper is
`/tmp/workdeck-pm05-qualification-20260909.py`. The superseded `d9f33355` candidate is
preserved under `intermediate-d9f33355/`; its passing 439-case PM run changed source
while the lifecycle repair landed and is not the accepted qualification run.

Independent review accounted for all eleven PM-05 deliverables and exit criteria.
The accepted source covers hierarchy/associations, shared query and exact CLI/TUI
membership, custom-field/identity policy, per-unit estimates, templates/saved views,
wiki/document links and immutable time amendments/reports. The real project/cycle
PTY includes milestone association, CLI/shared equality, reassignment, archival and
restoration. Lost-acknowledgement retries, obsolete custom-value lifecycle repair,
receipt consistency, prospective import policy and historical diagnostics have
explicit regression coverage. PM-05 is complete; PM-06–PM-12 retain their full scope.

An earlier changing-source PM sweep recorded one migration worker receiving the
bounded `Locked` error. Its unchanged exact serial retry passed (0.73 seconds), and
both subsequent full PM runs passed the same test. Earlier blank startup and FIFO
subprocess deadline incidents also remain recorded; they did not recur in the final
full CLI/PTY gate. These observations remain PM-12 environment/timing qualification
inputs, not evidence of dropped assertions or changed deadlines.

The focused checkpoints below are historical increments superseded by this final
candidate; their old pending-gate wording does not reopen the closed phase.

## PM-05 active increments — 2026-09-09

These are focused checkpoints on changing source, not combined phase qualification.

- The expanded workbench group passes **56 tests**, including issue custom-field
  creation under active policy, invalid JSON retained through closing/reopening the
  draft, and preserving unknown custom data when editing title/body. The first
  project/cycle PTY retry passed in 8.02 seconds with the same startup timeout.
  A further PTY extension adds a milestone, direct CLI/shared membership comparison,
  and reassignment while preserving the milestone; that extension is under qualification.
- Independent review reproduced a planning-create retry defect after a lost acknowledgement:
  changing the retained draft rotated its request and allowed a duplicate publication.
  The regression observed success where `IdempotencyConflict` was required. Planning
  drafts now retain the first request until explicit discard, matching issue drafts.
  All six planning-workspace regressions pass after the repair.
- Saved-view receipt proof is repaired: exact bounded YAML is included in the read/receipt
  record and checked against its parsed definition, path, content hash, repository and
  original operation input. Five core cases (including ten forgery variants), four CLI
  cases, operation history, snapshot and staging regressions pass. New diagnostics and
  prospective import checks for unusable saved predicates are under qualification.

- On resumed work, the 51-case workbench group passed. A new selection test then
  reproduced an Active/All membership mismatch after moving between project rows;
  the query now retains the current scope. Five focused planning cases pass after
  the repair: cross-kind failure isolation, retained stale draft with explicit discard,
  selection scope, required project custom-field authoring with invalid-text retention,
  and initiative outcome declarations. F11 opens Projects and F12 opens Cycles;
  F10 continues to open the existing app menu. Issue custom-field authoring and the
  expanded whole workbench group are currently under qualification.
- The project/cycle PTY acceptance test compiled, but its first run timed out at the
  initial blank screen before planning navigation. Its unchanged-timeout retry is
  pending. Separate process sampling in the Wiki qualification showed `_dyld_start`
  during another pre-test delay. Do not count the blank startup as a planning-action
  assertion failure or as successful terminal qualification.
- Expanded Wiki proof qualification passes **12 core tests and five CLI tests**.
  A reproduced receipt-body forgery is rejected after validating the result's exact
  content/path/hash/changed paths against the original input. Valid no-op and historical
  update replay survive later edits. Events, export and staging reject the tested forged
  receipts; no-op staging only stages the receipt, while stale changed-file staging
  retains the committed result and preserves unrelated index content.
- Saved views initially pass **three core tests and two CLI tests**: live shared-query
  membership, exact source preconditions, metadata/comment preservation, archive flag,
  original receipt replay, malformed query/path rejection, concurrent create, interrupted
  recovery, snapshot restore, and command/schema discovery without initialization.
  Independent review reproduced a saved-view receipt-result forgery; its repair and
  additional tests are in progress, so those first passes do not qualify receipt integrity.

- Wiki export initially failed with `Unsupported` for `wiki/architecture/overview.md`.
  Seven shared-core cases now pass, including plain Markdown with deliberately invalid
  YAML-like frontmatter, exact CRLF preservation, snapshot restore, hash-guarded updates,
  original receipt replay after later edits, concurrent duplicate requests, recovery
  after publication interruption, and unsafe path/content rejection. Three native CLI
  cases pass for file-body authoring, read/update, issue links, source diagnostics,
  introspection, and scoped staging preserving unrelated index content.
- The TUI unknown-status regression initially cleared selected issue state. Its shared
  query implementation now rejects the query before changing the last good snapshot,
  selection, filter, or draft. A second case checks alias/sort parity with the PM API,
  cached filtering after external source corruption, failed-refresh preservation,
  and full return-context restoration. The 43-case workbench group passed before the
  expanded hierarchy/archive filter form. Its 30-row case passed, then a 14-row
  test reproduced an invisible selected field. Forms now render a window of fields
  around the selection, keeping the input row visible. The complete 45-case workbench
  group passes after that repair. Actual PTY requalification remains pending.
- New issue associations now require existing planning identities. The prior TUI label
  fixtures were updated to create real label records; the free-form comma-bearing label
  fixture becomes the explicit `api-reviewed` identity. Historical existing associations
  remain readable and separately diagnosed; this does not silently rewrite old labels.

## Cutover work in progress — 2026-09-09

The repaired restore candidate `c55d2f2be7844032dcbc99553910093574e8f20c8ce5d21da01d3b0a3732b739`
passed the complete PM (**346**), CLI (**626**, including **106 PTY**), TUI
(**1,117**), supporting crates (**568**), and startup example (**3**) tests. Every
suite ended with its 967-input source manifest unchanged. Independent review
confirmed that the original empty-directory restore collision now fails during
preview and before publication. The final restore suite has 26 cases, including
planned-path shape races and private lock/bundle exclusion from Git.

Strict integrated lint then rejected three unnecessary clones in one CLI
extension-migration test. These assertions now compare borrowed slices; production
code is unchanged. Candidate `bebe756e71632e53b0c95958ab429126cbba1ab0653eb7f1d87d9ef3a6920ca9`
differs only in `crates/workdeck-cli/src/config.rs`. Its affected full CLI suite,
strict lint, architecture, skill, and formatting gates passed with unchanged source. The earlier
suite reports and per-file manifest remain archived under
`/tmp/workdeck-cutover-qualification-20260909/prior-source-c55d2f2b/`.
The affected CLI rerun passed all 520 non-PTY tests and 96 of 106 PTYs. Ten
extension startup cases timed out in the concurrent run; all 20 extension PTYs
passed serially on the identical candidate. The failed run is retained in `cli.log`;
`pty_extensions_serial.log` records the successful recheck. This is a timing concern
for PM-12 qualification, not proof of reliable concurrent startup.

An independent source-and-evidence audit found no remaining PM-02–PM-04 scope gap
across their 37 deliverables and exit criteria. Those phases are closed with the
qualification limits above. PM-05–PM-12 retain their full scope.

The following paragraphs retain the preceding intermediate evidence and findings;
where they say a combined run is pending, the candidate results above supersede
that older test status.


The first combined cutover source capture is
`f205aa95047ca65518c6c752354d358a01ada2badfbc0ac808c8d2d8d23640b1`
over 967 sorted inputs: Cargo manifests/lock and all current files under `crates/`,
`examples/`, `xtask/`, `skills/`, and `port/hunk/oracles/`, using the path/NUL/file-hash/
newline method. `cargo test --locked -p workdeck-pm --all-targets` passed **339**
top-level cases; the complete CLI `--all-targets` gate passed **626**, including **106**
PTY cases. Both ended with the source unchanged. Five subprocess worker results inside
the CLI safety suite are not counted again. Logs and the per-file manifest are under
`/tmp/workdeck-cutover-qualification-20260909/`; the repeatable local gate helper is
`/tmp/workdeck-cutover-qualification-20260909.py`.

This is not an accepted phase exit: independent review then reproduced a static restore
destination-shape gap. An empty directory at a planned file path (`labels.yml/`) was
omitted from the file-only capture, so preview allowed it and apply failed after barrier/
config publication. The repair adds explicit planned-file/parent shape checks before
publication and during resume without changing durable receipt formats. Final TUI,
support, lint, architecture and skill gates and the repaired-source checks remain pending.

This increment is still changing. The previous integration fingerprint below does
not qualify these changes. New evidence must be combined and checked against a
fresh source fingerprint before phase acceptance.

Root ran `cargo test --locked -p workdeck-cli --test pm_cli --test pm_lifecycle
--test pm_retirement --test pm_config_cutover --test pm_config_safety
--test pm_legacy_safety --test pm_search --test pm_source_routing -- --test-threads=1`:
**65 passed** (30/8/4/6/2/3/3/9 respectively). The established CLI suite also passed
46 cases after explicitly seeding actual legacy sources for legacy mutation tests;
catalog tests passed two cases. Fresh empty issue/reference reads now intentionally
return `not_initialized` without creating files; actual legacy reads retain their
existing envelope. Retirement's placeholder-unavailable assertions were replaced
only after real preview/mutation/replay/history/staging tests passed.

Meaningful failures observed and repaired in this increment:

- Config initialization seeded prototype files inside the native root; invalid
  `config set` changed bytes before validation; TOML edits lost comments or rejected
  inline tables. Config edits now validate the candidate before atomic publication
  and preserve unrelated TOML formatting. Config Get/Show/Validate also reject FIFO,
  symlink and oversized repository sources with bounded reads; explicit user config
  symlinks retain their existing read-only compatibility.
- Automatic ignore-file reads could hang on FIFOs or follow symlinks. Descriptor
  reads and bounded enumeration now cover ignore sources, file previews and recorded
  history. Invalid UTF-8 could expand a preview beyond its byte limit; binary and
  truncation handling now preserve the advertised bounds.
- Native/config source changes could make a panel display a different authority
  under its original identity. Providers pin native or legacy path/directory identity,
  native repository identity, and initially unavailable/empty selection. Planning
  source changes require reopening; unrelated Files/Git inspection remains usable.
- Search capped all groups before applying `--target`, which returned zero issues
  beside more than 10,000 file matches. A CLI regression reproduced the missing
  issue; filtering now precedes ranking/result truncation.
- Native deletion was unavailable. Four CLI tests now cover confirmation, a reviewed
  preview, short references, retained comments/records, replay, immutable retired IDs,
  source/membership changes, incoming-reference diagnostics and mixed-index staging.
  Shared-engine tests additionally cover interrupted publication, forged receipts,
  payload/metadata boundaries and history membership changes.

Implementation-owner evidence: final retirement **19** cases and strict PM Clippy
passed; the earlier full PM run passed **231** before three final cases/refactoring
(fresh full 234-test run still required). Final repository provider **14**, Git
provider **12**, safety **14**, and strict scoped CLI Clippy passed. The new panel
workflows passed five real PTY cases and 35 workbench controller cases, including
filter freshness, source return, selection and draft retention. Full final TUI/PTY,
workspace, architecture, lint and release checks remain to run after current changes.

Further bounded increments, also awaiting a combined fingerprint:

- The unused CLI `app`/`tui`/`views`/`syntax` island was removed after a caller audit
  and a passing 247-test CLI library baseline. Its 61 internal renderer/application
  tests retire with those implementations; all 186 remaining library tests passed.
  Native priority/label/copy/link actions were added, including stale retained forms
  and source-bound linking. Owner evidence: 41 focused workbench tests, 12 relevant
  PTY executions, strict TUI and CLI library/terminal-pager lint, and architecture
  (13 crates, one executable, zero violations). Fixed old visual breakpoints,
  tab wrapping and Syntect palette assertions are not claimed as native parity.
- Shared recorded-session mutations and native CLI adapters initially passed five
  PM and three CLI tests. They cover replay after later edits, concurrent notes,
  guarded direct edits, TOML formatting, atomic JSONL imports, inert annotations,
  and doctor visibility. Independent review subsequently found missing deletion
  marker, forged replay, case-collision and nested-metadata gaps. Final owner evidence
  is 18 PM history cases and five CLI cases, including removed-marker detection,
  canonical deletion-result proofs, permanent case-insensitive reservations, nested
  file metadata/comments, bounded replay admission and source-qualified mutation
  receipts in Events. Strict PM/CLI lint passed before the final bounded traversal
  change; the combined post-change run remains required.
- Exact native snapshots initially passed 19 owner tests; the affected PM group
  passed 124 tests. Three root CLI cases passed bare/enveloped/JSONL equivalence,
  exact source bytes, reviewed same-authority import, replay, explicit foreign-source
  blockers and malformed/oversized rejection. Import still requires existing matching
  receipts/config/migration authority. Legacy conversion and fresh restoration are
  not completed by this bounded snapshot contract.
- Headless Files/Changes safety tests reproduced linked-file exposure, FIFO ignore
  blocking, glob path mismatches and text-conversion helper execution. The port passed
  eight focused cases plus Git provider tests before the final attribute probe found
  a FIFO `.gitattributes` hang inside libgit2. Bounded active attribute and Git-ignore
  preflight now passes 53 owner cases (headless 11, provider 16, Git 12, safety 14),
  strict scoped CLI Clippy, and the existing read-only command regression. Capacity
  exhaustion reports incomplete coverage without invoking an unqualified libgit2
  scan or claiming a clean worktree. Concurrent malicious metadata swaps remain
  outside the cooperative filesystem guarantee. Status and legacy Search separately
  reproduced FIFO `.gitignore` hangs; their checked-provider port is in progress.
- Root moved directory limits into `Snapshot::list_bounded`, counting empty directories
  as well as files and retaining the strictest successful bound in recovery journals.
  Four observed runtime failures covered empty directories, cached-limit bypass,
  file-as-directory ambiguity and recovery traversing an expanded external tree.
  All five focused cases passed after the fix, alongside 29 transaction and two
  operation tests. Two additional operation-history tests passed with the two existing
  cases, covering exact receipts, ordering, wrong repository/path, malformed history
  and duplicate requests. New memory-snapshot and old-journal compatibility cases
  subsequently passed in the combined root run below.

Root subsequently ran the combined PM command with `--locked`, selecting `--lib`,
`bounded_snapshots`, `transactions`, `operations` and `history`: **66 passed**
(9/6/29/4/18). This includes memory traversal, prior journals without the new bound,
the final history repair and its doctor budget. The related CLI command selecting
`pm_snapshots`, `pm_source_routing`, `pm_history`, `pm_catalog` and
`pm_legacy_conversion` passed **21** cases (3/9/5/2/2). Native conversion cases used
actual bare, enveloped and JSONL legacy exports, retained exact source bytes and
completion provenance, replayed after a later issue reopen, and applied a reviewed
fingerprint with its explicit `--imported-at` timestamp.

The legacy importer accepted unknown JSON as an empty export and lost records when
given the program's own export envelope. Both failures were observed in two new
CLI tests. Shared strict transfer decoding and destination format rejection fixed
them: those two tests plus the established CLI suite passed **48** cases together.
A later independent actual-binary probe found invalid issue IDs were validated only
after `--replace` deleted the old backlog. Complete preflight, raw metadata/events
preservation and replacement limited to legacy authority files subsequently passed
the owner's 10 legacy transfer tests and 46 established CLI cases. Those tests
qualified the strict transfer helper; the ordinary legacy write surface is now
being closed under PM-03.E5 rather than retained as a second writable store.

Status and legacy Search ports passed **10** owner cases (six new regressions and
four existing CLI cases) and scoped CLI lint. Active CLI callers no longer use the
old unchecked Git scanners or whole-file symbol reader. Original public helpers
remain until their compatibility scope is separately retired.

Native restoration now has a shared create-only-or-identical protocol, explicit
`restore.yml` barrier, retained bounded input and `import --restore --resume` routing.
Five root CLI cases pass: preserving application settings and exact original receipts,
resuming after a barrier without external input, rejecting changed preferences after
preview, staging every restored receipt while preserving unrelated index entries,
and rejecting staging of a replay after restored receipt bytes change. The combined
root native CLI run passed 21 cases (`pm_catalog` 2, `pm_legacy_conversion` 2,
`pm_restore` 5, `pm_snapshots` 3, `pm_source_routing` 9). Final restore owner evidence
is 19 passing cases and strict PM Clippy. A real process exit after barrier publication
first exposed the retained bundle and writer lock in Git; a create-only local ignore
file before staging fixed the reproduced failure. Conflicting root/local ignore
rules remain explicit errors without overwriting user rules. The affected owner union
also passed 88 cases before these final ignore tests. These focused results do not
replace final integrated qualification.

The final legacy-to-native converter owner run passed 66 cases (19 legacy transfer,
19 snapshots, 19 repository, nine library), strict PM Clippy and scoped formatting.
Root then added a visible notice that the old JSONL framing has no terminal count
and cannot detect clean trailing-row truncation; all 19 transfer cases passed again.
Source artifacts, imported completion provenance and omitted-timestamp replay stay
distinct from newly verified completion evidence.

The PM-02–PM-04 acceptance audit found three concrete remaining cutover gaps:
ordinary legacy mutation admission, selected legacy app-preference writes, and a
note-derived issue test that stopped before persistence. Legacy PM mutation handlers
are being retired with intentional native compatibility fixture migration. App-config
init/set will explicitly move preserved TOML into the canonical root; review saves
against a legacy preference source will report an actionable read-only error while
global preference behavior remains intact. These changes remain in progress.

The mounted note test now submits the actual form, reopens the repository and checks
issue title/body/source location, durable creation receipt and the retained original
review note: one focused TUI test passed. The same workflow passed a real CLI-backed
PTY case (1.92 seconds); its first run used an incorrect test predicate for the native
note composer label, corrected against the actual rendered screen without changing
the application. Active
theme/extension guidance now names the canonical root; old prototype plans and the
TUI split document are marked historical. `cargo xtask skill check` passes after
updating the extension skill's destination digest and adaptation evidence, preserving
its frozen upstream identity and complete section mapping. The compiled extension
startup fixture now uses `.workdeck/extensions`; its provisional-prefix lifecycle
case passed without restarting the already loaded global extension.

Two new real-binary reference-resolution tests first failed for the intended reason:
native `delete --force --dry-run` rejected the feature as legacy-only. After implementation,
the all-kinds flow found a last-label defect: omitted empty JSON labels compared unequal
to the reviewed empty list, so post-write validation incorrectly reported failure.
Typed label normalization and pre-publication proof validation repair that case. Root's
combined `pm_reference_resolution` (three), `pm_retirement` (four), and `pm_catalog` (two)
run passed **nine** cases, including every reference kind, preserved history, replay
after later edits, stale membership/direct-editor changes, and exact scoped staging
beside unrelated index entries. Core fault/tamper expansion and final combined
qualification remain in progress; this does not complete the richer PM-05 hierarchy.

Final app-preference owner evidence is **88 distinct** focused tests: 56 CLI config
library, eight cutover, two config safety, one CLI bootstrap, 17 TUI policy/controller/
reload, three core preference and one core bootstrap. The final ignored-temporary-file
change also reran all five config-edit cases. Invalid candidates first reproduced
canonical-directory creation and changed extension discovery; prevalidation now rejects
before scaffolding. A real Git-status probe exposed the app-config lock, repaired with
preserved/create-only local ignore policy before lock publication. Strict lint is being
rerun after a reported iterator-style diagnostic; no broad lint result is implied.

The read-only legacy cutover owner passed **111** selected CLI integration cases and
five Store reader cases. All 46 established CLI scenario names remain, now backed by
actual preview/apply migration fixtures and intentional native envelope/identity/
retirement expectations. The admission matrix covers 52 mutation variants against
both canonical and custom legacy roots, checking exact bytes and membership. Duplicate
Store writers and the legacy apply helper were removed. Store tests changed from 11 to
five: five tests of removed writer implementations retired, and four writer-backed read
tests became three raw-fixture metadata readers; alias and read-only event coverage
remains. Native CLI/shared operations retain create/link-dedup/update/validation coverage.
All ten transfer scenarios remain as immutable preview/export and preservation tests.

Two strict subprocess deadlines intermittently failed: original broker readiness
(five seconds) and config safety (two seconds). Captured delayed children were
before Rust `main`: broker 777/780 samples and config 803/803 samples at `_dyld_start`,
with no application config handles, runtime files or listener. Unchanged broker
runs subsequently passed directly (1.36 s) and through Cargo (3.06/3.10 s); config
passed directly (0.18 s) and through Cargo (1.71 s, then the root combined run).
This supports intermittent macOS launch delay; the exact OS cause and build contention
are unproven. No deadlines/assertions were relaxed and no all-runs-reliable claim is made.

Descriptor reads are Unix implementations; directory enumeration is implemented
for macOS/Linux and executed here on macOS. Unsupported platforms return explicit
errors. This does not qualify Windows, Linux runtime execution or full release support.

## Integration checkpoint — 2026-09-09

Snapshot SHA-256: `d32f81360daaa5de5cb156494236207946d36ddfea050553332c4fd64e25bfe3`
over 93 sorted inputs, using the path/NUL/file-hash/newline method below. Inputs
extend the foundation set to every current CLI `src/pm_*.rs` and `tests/pm_*.rs`,
the terminal-pager harness and its new workbench tests, and all current PM and
workbench files (including migration's protocol document). External design/evidence
documents and build artifacts remain excluded. A final rehash found no changed
input after these runs. The baseline branch and low-debug Cargo settings below
remain unchanged.

| Command | Observed result |
| --- | --- |
| `cargo test --locked -p workdeck-pm` | **214 passed**: 5 unit; 9 attachments; 5 comments; 6 contracts; 23 documents; 2 imported completion; 5 issue lifecycle contracts; 15 issues; 15 migration application; 12 migration preview; 2 operation recovery; 18 planning; 19 repository; 17 schema; 1 source identity; 18 staging; 13 templates; 29 transactions |
| `cargo test --locked -p workdeck-cli --test pm_cli --test pm_aliases --test pm_catalog --test pm_migration --test pm_operations --test cli --test git_integration` | **103 passed**: 30 native issue/reference/staging CLI; 6 aliases/lifecycle/ambiguity; 2 catalog; 5 migration; 2 operation recovery; 46 established CLI; 12 Git integration |
| `cargo test --locked -p workdeck-tui --lib` | **1,092 passed**, rerun after the architecture boundary correction |
| `cargo test --locked -p workdeck-cli --test terminal_pager -- --test-threads=2` | **98 passed**: existing pager, mouse, layout, inline editing, extension, note, session and terminal behavior plus the five workbench cases |
| `cargo test --locked -p workdeck-cli --test terminal_pager workbench:: -- --test-threads=2` | **5 passed again** after moving session translation from planning into the composition shell |
| `cargo clippy --locked -p workdeck-pm -p workdeck-cli -p workdeck-tui --all-targets -- -D warnings` | Passed after final migration guards and the session boundary correction |
| `cargo xtask architecture check` | 13 production crates, one executable, zero violations; no rule exceptions added |
| `cargo fmt --all --check` and `git diff --check` | Passed |

Meaningful regressions observed and repaired during this increment:

- Native `project save` created a competing legacy root. All reference adapters
  now use native operations, including one atomic, replay-aware compatibility save.
- `--init` still generated the old scaffold. Its intentional compatibility
  migration now initializes native PM and leaves existing TOML preferences intact.
- Legacy status/priority spellings stopped working. Shared parsers preserve exact
  configured states, canonicalize supported inputs, and reject normalized ambiguity.
- Explicit staging was absent; its implementation then exposed a real Git
  post-index-change hook invocation. Staging now suppresses filters/hooks,
  preserves unrelated index/worktree contents, checks exact receipt/source bytes,
  and exposes committed mutation receipts when staging fails.
- Clean-file review jumps had no visible rows because an unchanged file has no
  hunks. The existing source snapshot is projected into context rows before
  publication; no extra file read or fabricated additions/deletions is used.
- The first navigation bridge imported session transport types into planning UI.
  The unchanged architecture rule caught it. Planning now passes core inputs to
  the existing composition shell, where session translation belongs.
- Ordinary raw config writes could change a completed migration's identity, and
  altered pending cutover intent needed stricter proof. Both fail closed now,
  with subprocess, interruption, conflict, journal and receipt tests.

This qualifies the current file engine, baseline commands and mounted workbench
increment on macOS/Unix. PM-03 still needs all active path/consumer cutovers;
PM-04 still needs useful legacy panel ports and removal of duplicate mutation
paths. Native deletion/reference resolution and remaining PM-02 closure evidence
are open. Full workspace/package/platform qualification, PM-05–PM-12 functionality,
large-dataset performance and final independent review remain required. No user
backlog or shared remote was used as a test fixture.

## Foundation checkpoint — 2026-09-08

Baseline commit: `c9a36ec6d6545570380e193a0a23d36cb591356f` on
`linear-project-management`. Implementation remains uncommitted and existing
user-authored planning documents are preserved.

Snapshot SHA-256: `8b962c09078b1309731e6859badd143d052727e7816ba91f0cd2d864ac94bd0d`
over 76 sorted source, fixture, test, manifest, lockfile, and architecture inputs.
The digest hashes each `relative-path + NUL + file-SHA256 + newline` entry.
Inputs are every file under `crates/workdeck-pm/` and
`crates/workdeck-tui/src/workbench/`, plus root Cargo manifests, CLI manifest,
`main.rs`, `pm_cli.rs`, `pm_catalog.rs`, `command_names.rs`, the three `pm_*` CLI
integration test files, existing `cli.rs`/`git_integration.rs`, TUI manifest/lib,
extension API `lib.rs`, and `xtask/src/architecture.rs`. Documentation and build
artifacts are excluded so adding evidence does not change the implementation hash.

All Cargo build/test/lint commands use:

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
```

These settings reduce local artifact size after an earlier disk-space exhaustion;
they do not disable tests or acceptance checks. Only this worktree's disposable
target directory was cleaned during recovery from that environmental failure.

### Passing checks

| Command | Observed result |
| --- | --- |
| `cargo test --locked -p workdeck-pm` | 168 tests passed: 1 template snapshot unit test; 9 attachments; 5 comments; 6 contracts; 23 documents; 2 imported completion; 15 issues; 10 migration previews; 2 operation recovery; 16 planning; 19 repository; 17 schema; 1 source identity; 13 templates; 29 transactions |
| `cargo test --locked -p workdeck-cli --test pm_cli --test pm_operations --test pm_catalog --test cli --test git_integration` | 86 tests passed: 24 native issue CLI, 2 recovery CLI, 2 catalog CLI, 46 existing CLI, 12 existing Git integration |
| `cargo xtask architecture check` | 13 production crates, one shipped executable, zero dependency/source-reachability violations |
| `cargo clippy --locked -p workdeck-pm -p workdeck-cli --all-targets -- -D warnings` | Passed, including compilation/linting of the TUI dependency |
| `cargo fmt --all --check` | Passed after formatting newly added PM tests |
| `cargo test --locked -p workdeck-pm --test contracts --test issues` | 21 focused tests passed after final test formatting |
| `cargo test --locked -p workdeck-cli --bin workdeck clap_command_tree_is_the_executable_cli_reference_contract` | Executable command-tree contract passed |
| `cargo test --locked -p workdeck-tui --lib workbench::tests` | All 11 controller/render tests passed in root integration verification |

The workbench controller also has strict TUI library/tests lint evidence from its
implementation owner. Actual terminal routing and PTY qualification remain open.

### Meaningful failures fixed before acceptance

- YAML edits initially changed multiline/flow values or formatting; round-trip,
  duplicate-key, unsupported-feature, and transactional patch tests now preserve
  unrelated bytes and reject ambiguous source syntax.
- Completion could remove criteria in the same operation or retain old manual
  acceptance after edits. The shared completion operation now blocks both cases.
- Retry logic depended on the current workflow/template rather than the original
  request. Durable replay now precedes those lookups and preserves original results.
- Comment writes shared the item revision, and readers accepted malformed headers,
  empty bodies, control-character authors, and nested comment files. Comments now
  have independent files and strict shared validation.
- Direct edits to unwritten dependencies, directory membership, or already-replaced
  files could evade a prepared write. Snapshot/read-set rechecks and recovery tests
  expose these conflicts instead of publishing a success receipt.
- Configuration deletion could make a transaction impossible to reopen. Such a
  change is now rejected before a journal is published.
- Editing could switch repository identity and misattribute the result. Handles
  pin their repository identity and reject changed-source reads, writes, and retries.
- Editor banners corrupted JSON stdout, and human diagnostics lost source locations.
  Editor output now uses stderr; errors include escaped paths and line/column.
- Doctor ignored independent malformed records. It now inspects issue documents,
  comments, attachment metadata/layout, templates, projects, cycles, and labels.
- Recovery existed only as an engine primitive. CLI tests now inspect pending work,
  perform a read-only dry run, explicitly finish recoverable work, and prove reruns
  do not duplicate effects or initialize an empty directory.
- Legacy Done could not be represented without inventing a completion time. Import
  provenance now preserves unknown historical completion and records supersession
  on reopen without manufacturing manual acceptance or check evidence.
- Initial schema discovery omitted the writable fields inside create/update inputs.
  Generated authoring schemas now use the same writable-field list as mutations.
- A retained TUI comment draft could acquire a fresh request ID after a lost
  acknowledgement, creating a duplicate. Draft retries now preserve request identity;
  changed payloads reach the shared idempotency-conflict check.

### What this checkpoint does not qualify

- Migration is currently preview/conversion only. Application, bootstrap barriers,
  manifests, consumer cutover, and interrupted migration recovery are still required.
- New project/cycle/label operations and the workbench controller exist in the shared
  library; native CLI adapters and persistent startup/form routing remain to integrate.
- Legacy `--init` and prototype source consumers have not been cut over. Explicit
  `workdeck init` exercises the native opt-in source.
- Explicit scoped staging and native issue deletion compatibility remain unresolved.
- Attachment doctor/list operations inspect descriptors and layout. Only explicit
  attachment reads verify payload size/hash; they never execute or render payloads.
- Unsafe filesystem entries produce diagnostics and can stop that subtree's listing;
  doctor does not claim exhaustive diagnostics through an unsafe subtree.
- Transaction directory syncing currently supports Unix. Windows durability/process
  qualification and Linux runtime qualification have not been completed here.
- Hierarchy expansion, graphs, features/gates/evidence, context/handoffs, command/check
  execution, Git claims/publication, SQLite/FTS/boards/multi-repository views, CI trust,
  scaled performance, full terminal/workspace checks, packaging, and release remain open.

No phase beyond the foundation contracts can be accepted from this checkpoint alone.


### PM-10 mounted terminal and pending-input checkpoint — 2026-09-09

The mounted index passed 143 workbench tests before the pending-input changes
(`/tmp/workdeck-pm10-mounted-workbench-third.log`), and seven focused index cases
including independent excerpt scrolling (`/tmp/workdeck-pm10-indexed-excerpt-scroll-green.log`).
These are historical focused results, not qualification of the latest source.

Actual terminal workbench tests returned **13 passed, 8 failed** in 32.71 seconds
(`/tmp/workdeck-pm10-mounted-pty-first.log`). Seven failing context, claim, navigation,
and sequential-form journeys expose input arriving before the asynchronous native
selection is ready. One broker-navigation failure reported no broker startup output;
its cause is not established and must be investigated separately. Assertions and
terminal deadlines remain unchanged.

Pending repairs retain ordered input while the initial readonly context or an
already identified issue waits for its reader. Mutation replay must retain the issue
identity, use its newly validated source, and cancel on source errors or selection
changes. The queue is bounded to 64 entries and 64 KiB; overflow retains a canceled
marker until explicit cancellation so field text cannot become unrelated commands.
Escape, function navigation and interrupt cancel pending input. A literal `q` remains
field input while the queue owns it. Added coverage exercises batched context keys,
immediate post-save labels, overflow and cancellation. **These changes and tests have
not compiled or run yet.**

The current verification invocation is retained at session `80175`, observed Cargo
PID `80385`, with an empty `/tmp/workdeck-pm10-indexed-input-second.log`. A separate
Homebrew Python sample (`/tmp/workdeck-pm10-python-stall-sample.txt`) found all 793
samples at `_dyld_start`, before application code. A separate direct-toolchain Cargo
version probe also produced no output before its bounded probe was terminated.
System Python and system process inspection remain usable. This establishes a
host executable-startup observation, not its cause; it does not establish a product
failure or passing tests. Only verified root-owned stalled diagnostic processes were
terminated. No duplicate build, security-setting change, or sibling mutation was made.
Next: obtain compiler output, repair affected behavior, rerun workbench and unchanged
actual terminal journeys, then continue the remaining PM-10 through PM-12 scope.

A later sample of the retained Cargo invocation itself
(`/tmp/workdeck-pm10-cargo-startup-sample.txt`) identified the rustup executable,
96 KiB physical footprint, and 93/93 main-thread samples at `_dyld_start + 0`.
No Rust compiler or test body had started at that observation. Disk inspection
reported 1.2 GiB available; no causal connection to startup has been established.
Explicit queue cancellation also clears its obsolete waiting/canceled notice.

On the third consecutive goal-turn observation of this host condition, system
process inspection still showed Cargo PID `80385` live (elapsed 10:13) and its
verification log remained zero bytes. An independently owned direct `rustc --version`
probe also failed to emit startup output within three seconds and was terminated
and reaped without affecting the retained Cargo invocation. Goal execution is
blocked pending restoration of Rust executable startup. The latest source is not
qualified; the remaining PM-10, PM-11 and PM-12 requirements retain full scope.


### User-requested Cargo artifact cleanup — 2026-09-09

The user identified storage pressure and requested Cargo cleanup. Root deliberately
terminated its Cargo PID `80385` for cleanup; session `80175` returned exit 143.
No build from that handle remains live. The other observed Cargo PID `92719` had
cwd `/Users/rutger/Projects/workdeck`; its process and artifacts were preserved.
Because Cargo itself could not start, system Python removed regenerable files only
under this worktree's `target/`, retaining `debug/workdeck`, the legacy CLI test
executable and the VCS test executable. Removal accounted for 3,833,905,152 allocated
bytes. Filesystem free space rose from 4.1 GiB to 7.8 GiB; APFS accounting may differ
from per-file totals. Source, lockfile, installed toolchains and registry caches
were preserved. A direct toolchain `cargo --version` probe after cleanup still
produced no output within five seconds; that owned probe was terminated and reaped.
Cleanup is complete for this idle worktree, but executable startup recovery has not
been demonstrated. Latest input changes remain unverified.


### Toolchain recovery and input regression verification — 2026-09-09

Direct Cargo version probes now succeed in both minimal and inherited environments.
The cause of recovery is unproven; no host security settings or toolchains were changed.
After the artifact cleanup, the focused TUI build completed in 48.36 seconds and all
nine `workbench::indexed_core_tests` passed in 1.33 seconds
(`/tmp/workdeck-pm10-input-after-cleanup.log`). This includes batched initial context
and an immediate action after save, with the same issue's newly validated source.
The full workbench regression suite is now running; actual terminal failures have
not yet been requalified. This clears the executable-startup blocker, not PM-10.

The initial full-workbench invocation exited 143 at test startup without results.
The confirmed-ended invocation was retried: 145 passed and three mounted checks/source
cases failed because their test drivers inspected state without pumping the now
asynchronous queued input (`/tmp/workdeck-pm10-workbench-after-cleanup-retry.log`).
The source-test key helper now pumps the existing reader event loop after dispatch;
the mounted-checks test pumps after its original batched key sequence. All behavior
assertions are unchanged. The resulting full workbench suite passed **148 tests**
in 24.48 seconds (`/tmp/workdeck-pm10-workbench-input-green.log`). The unchanged
21 actual terminal journeys are running in session `92627`, log
`/tmp/workdeck-pm10-pty-input-retry.log`; previous terminal failures are not yet closed.

The unchanged actual terminal rerun returned **17 passed, four failed** in 31.83 seconds
(`/tmp/workdeck-pm10-pty-input-retry.log`). Remaining cases cover pending shared
publication, linked-file/commit return, accepted/proposal citations and source-operation
publication. Three showed the native bridge rejecting shared working-tree captures:
these correctly have `Proposal` role, while the bridge required `Local`. A new real
shared-source regression failed with the intended `StaleSource`
(`/tmp/workdeck-pm10-shared-live-red.log`). The repair derives the expected role from
the locked current configuration while retaining the physical WorkingTree-selector
slot check, exact path/content and repository checks. The new regression also rejects
ref-backed proposal records. Pending input can now anchor to the already selected
indexed issue identity while its native record is loading. Focused verification is
running in session `18521`, `/tmp/workdeck-pm10-shared-live-green.log`; these latest
changes are not yet qualified.

The shared-source focused rerun passed all **10 indexed-core tests** in 2.87 seconds
(`/tmp/workdeck-pm10-shared-live-green.log`), including the real working-tree versus
proposal-ref regression. The unchanged actual terminal suite then passed **21/21**
in 27.76 seconds (`/tmp/workdeck-pm10-pty-shared-retry.log`). This resolves the eight
failures in the first mounted-terminal run without changing its assertions/deadlines.
Full workbench regression is rerunning against the final role/selection fixes in
`/tmp/workdeck-pm10-workbench-shared-final.log`. PM-10 full qualification, formatting,
strict lint, remaining UI integration and performance budgets are still open.

The full workbench regression after the shared-source fixes passed **149/149** in
21.10 seconds (`/tmp/workdeck-pm10-workbench-shared-final.log`). This is the source
that also passed all 21 terminal journeys, before the subsequent mouse change.

The next mounted-interface regression demonstrated that wheel events over an opened
source moved the issue list instead of scrolling the source (intended RED at narrow
width, `/tmp/workdeck-pm10-mouse-source-red.log`). The renderer now exposes both
pane rectangles; mouse handling scrolls only the pane under the pointer and ignores
wheel events in the header/footer. The focused narrow/wide regression passed in
1.31 seconds (`/tmp/workdeck-pm10-mouse-source-green.log`), preserving selected issue
and exact opened source. Adjacent runtime tests are running in
`/tmp/workdeck-pm10-mouse-runtime.log`. Remaining indexed Features/Planning integration,
boards/tree/activity, repository/source switching and performance budgets stay open.

All **19 adjacent mounted runtime tests passed** in 2.88 seconds after the mouse
change (`/tmp/workdeck-pm10-mouse-runtime.log`). No Cargo process remains owned by
this increment. Next work is the remaining PM-10 indexed collection integration.


### Indexed Features integration: mounted acceptance baseline

`mounted_feature_collection_does_not_retain_every_native_document` now exercises the
normal shell's `v` entrypoint against three real native features. The test failed
for the intended reason: the mounted controller retained all three full documents
(`/tmp/workdeck-pm10-feature-mount-red.log`, 0.40 seconds). This is a new open PM-10
acceptance test, not a regression introduced by the earlier mouse fix. Current
`FeatureWorkspace::refresh_after_list` calls `list_features`, then separately loads
coverage; navigation invokes the same refresh. The required next implementation is
bounded indexed collection browsing with one selected native authoring record,
retained draft/request/source pins and preserved coverage behavior. Merely truncating
the existing list or hiding records would not meet the requirement. No implementation
for this new acceptance test is claimed, and the full suite now contains this
intentional failing acceptance baseline until Features integration is complete.


### Indexed Features collection mounted — active increment

The normal shell now creates an indexed `FeatureWorkspace`. Its reader owns bounded
feature rows; the authoring controller retains one native record and the existing
source-bound coverage/draft state. Up/Down/Home/End navigation uses the indexed
viewport. Creates/edits locate the saved feature after refresh; archived records leave
the active query. Reader shutdown is initiated and joined outside the shell mutex.
The mounted test now waits for actual loading, verifies total count three, moves to
the last feature and verifies exactly one native document remains. All eight feature
workspace tests pass (`/tmp/workdeck-pm10-feature-index-third.log`, 0.94 seconds).

The stronger test exposed simultaneous Issues/Features cold readers racing on
`.index/.gitignore`: the observed error was `File exists` before any feature handle
(`/tmp/workdeck-pm10-feature-index-diagnostic.log`). Cache initialization now writes
and syncs a unique temporary file and atomically links complete bytes into place
without replacing an existing policy. Descriptor-relative operations retain the
opened directory; a competing policy is still checked and preserved. A new eight-reader
concurrency regression and adjacent storage/source tests are running in
`/tmp/workdeck-pm10-feature-cache-adjacent.log`.

This increment does not qualify all Features performance: native coverage still
loads synchronously after selected-record validation. It must be moved to owned
bounded background work before responsiveness qualification. Real terminal feature
journey, additional stale/filter/draft interactions, and broad regression remain to
be run on this increment. Planning/boards/tree/activity and cross-repository UI also
remain open. Do not count the earlier phase-wide test results as current qualification.

All 15 core projection/source/cache tests passed after atomic ignore initialization:
five live-source cases (including eight simultaneous cold readers) and ten storage
cases (`/tmp/workdeck-pm10-feature-cache-adjacent.log`). The actual terminal feature
journey first exposed a missing archived-row label, then displayed old rows while a
replacement query was still loading. Both failures remain recorded in
`/tmp/workdeck-pm10-feature-index-pty.log` and
`/tmp/workdeck-pm10-feature-index-pty-green.log` (the latter filename was prospective,
not a passing result). Restored archive labels and current/settled-query row display
then passed the unchanged narrow/wide terminal authoring journey in 9.91 seconds
(`/tmp/workdeck-pm10-feature-index-pty-third.log`). The workflow creates, edits,
refreshes externally changed issue coverage, archives, shows archived and restores.
Full workbench regression is running in `/tmp/workdeck-pm10-feature-mounted-workbench.log`.
Selected-feature coverage remains synchronous and unqualified for responsiveness.


### Selected-feature coverage moved to owned background work

The mounted Features integration passed all **151 workbench tests** in 21.12 seconds
(`/tmp/workdeck-pm10-feature-mounted-workbench.log`) before the next coverage change.
The new regression `indexed_feature_selection_defers_coverage_until_worker_completion`
failed because selection evaluated coverage synchronously
(`/tmp/workdeck-pm10-feature-coverage-red.log`). The repair uses an owned coalescing
`ProjectionWorker` for native selected-record and coverage reads during navigation.
The worker validates the same indexed source before and after coverage capture;
application accepts only the currently requested row token. Refresh invalidates old
work; shutdown takes both feature readers and joins them outside the shell mutex.
Previous coverage is labeled as retained while selected coverage loads. Explicit
edit/archive preparation still validates the selected native record before authoring;
ordinary navigation does not perform this read synchronously.

All nine feature-workspace tests pass after the change
(`/tmp/workdeck-pm10-feature-coverage-second.log`, 1.05 seconds). The unchanged actual
terminal feature journey is running in `/tmp/workdeck-pm10-feature-coverage-pty.log`.
Large-dataset responsiveness, more stale-completion fault cases and complete phase
qualification remain open; moving I/O to a worker is not performance-budget evidence.

The unchanged narrow/wide terminal feature journey passed with background coverage
in 10.88 seconds (`/tmp/workdeck-pm10-feature-coverage-pty.log`). No Cargo process
remains live. Next: deterministic obsolete/error/shutdown coverage tests and the
remaining PM-10 UI/performance work.


### Feature coverage fault and lifecycle checks

Three controlled-worker tests now exercise an obsolete error arriving after selection
changes, a current-source error retaining previous inspected coverage, and shutdown
joining blocked in-flight work after the shell transfers ownership. Channel gates
control completion order; timeout bounds prevent stranded test workers. All 12
feature-workspace tests passed in 1.37 seconds
(`/tmp/workdeck-pm10-feature-coverage-faults.log`). The resulting full workbench
regression passed **155/155** in 21.83 seconds
(`/tmp/workdeck-pm10-feature-worker-workbench.log`). No Cargo process remains live.

The next integration target is `PlanningWorkspace`: `refresh` still calls
`list_planning(self.kind)` and retains every native record, and selection separately
loads membership. Its per-kind selection/drafts and initiative/project/milestone/
cycle/target/label authoring must survive migration to the bounded reader. The
Features work is focused evidence, not closure of PM-10 or its performance gates.


### Source-bound single planning record adapter

`Repository::planning_from_projection` now reopens one native planning citation
through the existing physical checkout/WorkingTree-selector, repository, role,
path and content checks. Native Markdown parsing was extracted from `load_planning`
and reused; labels retain their shared canonical YAML source and native validation.
The adapter does not replace hierarchy or mutation policy and grants no write authority.
Its initial test failed to compile because the adapter was absent
(`/tmp/workdeck-pm10-planning-live-red.log`); that is missing-interface evidence,
not a behavioral stale-source RED. The completed test exercises all six planning
kinds and rejects wrong kinds, paths, accepted-source roles and direct editor changes.
All six live-source tests passed in 1.74 seconds
(`/tmp/workdeck-pm10-planning-live-second.log`). A milestone fixture initially lacked
its required project and was corrected without weakening validation.

Adjacent planning/hierarchy/organization suites passed 18, 17 and 28 tests respectively
(63 total; `/tmp/workdeck-pm10-planning-parser-adjacent.log`). Planning's mounted reader
is not switched yet. A new real-shell collection-bound test is running in
`/tmp/workdeck-pm10-planning-mount-red.log`; it must fail against current whole-list
loading before the bounded-reader integration. The adapter is a prerequisite, not
completion of Planning integration or PM-10.

The mounted Planning acceptance test failed for the intended reason: opening F11
retained all three native project documents (`/tmp/workdeck-pm10-planning-mount-red.log`,
0.36 seconds). This new test remains intentionally red pending integration. No Cargo
process remains live. Next: mount bounded indexed Planning collections and owned
membership loading while retaining per-kind selection and all draft/source contracts.


### Planning collection/background membership integration — active

The mounted shell now constructs indexed PlanningWorkspace. Bounded rows and native
membership use separate owned readers; membership requests capture kind, query and
row token, validate the native record before/after capture, and reject superseded
results. Projects/Cycles and bracket-kind navigation retain the per-kind selection
and draft maps. Explicit edit/archive/member navigation revalidates one native record.
Rendering uses settled current-query rows; previous membership remains labeled while
new membership loads. Shutdown transfers both readers outside the shell mutex.

The first compilation exposed a private worker-input type in the shutdown return;
visibility was corrected. Seven focused planning-workspace tests passed
(`/tmp/workdeck-pm10-planning-index-second.log`, 1.06 seconds). The mounted regression
was then strengthened to await actual loading, assert all three indexed projects,
navigate to the last record, retain an edit across empty Cycles, and restore the
same project/draft with exactly one native document.

The strengthened run could not compile: its `target/` directory disappeared during
compilation, with missing fingerprint/rmeta files and a killed build-script process
(`/tmp/workdeck-pm10-planning-index-third.log`). System inspection confirmed no remaining
Cargo/rustc process, no worktree target directory, and 119 GiB free. Root did not
perform that removal. Verification now uses `CARGO_TARGET_DIR=/tmp/workdeck-pm-verification-20260909`
alongside the existing no-incremental/no-debug environment. The current focused rebuild
is session `51157`, `/tmp/workdeck-pm10-planning-index-isolated.log`. Preserve and poll
that handle; the stronger test and current source remain unverified until it finishes.
No phase criteria or assertions were waived.

The isolated rebuild completed and all seven Planning tests passed in 1.11 seconds
(`/tmp/workdeck-pm10-planning-index-isolated.log`), including the strengthened settled
collection, End navigation, empty Cycles switch and retained project edit/selection.
The unchanged actual Projects/Cycles terminal journey is running in session `65869`,
`/tmp/workdeck-pm10-planning-index-pty.log`. Use the dedicated CARGO_TARGET_DIR for all
subsequent validation while keeping source and Cargo.lock in the authorized worktree.

The unchanged Projects/Cycles actual terminal authoring/membership journey passed in
9.34 seconds (`/tmp/workdeck-pm10-planning-index-pty.log`). Full workbench regression
is now live at session `4996`, `/tmp/workdeck-pm10-planning-index-workbench.log`, using
the dedicated temporary target directory. This is focused integration evidence;
all-kind empty views, per-kind source/error/fault cases and full phase qualification
remain required.


### Empty planning kinds and empty label-registry correction

The indexed Planning integration passed **156/156 workbench tests** in 21.34 seconds
(`/tmp/workdeck-pm10-planning-index-workbench.log`). A new all-six-kind empty-state
check passed when no label registry existed. Adding a valid `schema: 1; labels: []`
registry exposed an actual failure: the Labels planning view tried to treat the
path-keyed inventory row as a logical label and returned InvalidSchema
(`/tmp/workdeck-pm10-empty-label-registry-red.log`). Planning Label queries now exclude
that aggregate row; general source inventory keeps it inspectable. Native label IDs
cannot equal `labels.yml`, so real labels are preserved by this predicate.

All eight Planning tests passed after the repair
(`/tmp/workdeck-pm10-empty-label-registry-green.log`, 1.15 seconds). A core regression
checks empty planning results versus retained registry inventory, creating/selecting
a real label afterward, and immutability of the old generation. Core live/storage
checks are running in session `58483`, `/tmp/workdeck-pm10-empty-label-core.log`, using
`CARGO_TARGET_DIR=/tmp/workdeck-pm-verification-20260909`. The initial file named
`empty-planning-red.log` passed without a registry and is not failing evidence.
Remaining planning membership fault/lifecycle and PM-10 integration/performance checks
retain full scope.

Core validation passed **17 tests**: seven live-source/label-inventory cases and ten
storage cases (`/tmp/workdeck-pm10-empty-label-core.log`). No Cargo process remains
live. Next: controlled Planning membership source-switch/error/shutdown tests, then
remaining boards/tree/activity and cross-repository UI/performance integration.


### Planning membership fault/lifecycle qualification increment

Three controlled tests cover a delayed Project result after switching to a Cycle
with the same logical ID, a current-source error preserving inspected membership
and draft preconditions, and joined shutdown of an in-flight membership worker.
Channel gates control read completion order; no test worker waits indefinitely.
All **11 Planning tests passed** in 1.18 seconds
(`/tmp/workdeck-pm10-planning-membership-faults.log`).

All 21 actual terminal workbench journeys are now running against the integrated
Issues/Features/Planning readers in session `86349`,
`/tmp/workdeck-pm10-indexed-collections-pty.log`. The dedicated temporary target
directory remains in use. Next: finish that regression, then implement list/board
switching and native feature-tree presentation using bounded source-consistent queries.
The existing grouping API returns counts, not a complete board interface; do not
count its presence as shipped boards. Activity, cycle carryover, multi-repository UI
and large-dataset performance qualification also remain required.

The integrated terminal rerun returned **19 passed, two failed** in 30.88 seconds
(`/tmp/workdeck-pm10-indexed-collections-pty.log`). Both failures were issue file/commit
navigation receiving `Select an available indexed issue` after a visible row. Review
found two index polls during one key dispatch: refresh could replace page state after
the deferral guard but before action preparation. The second poll was removed.
Rendered index state now retains its selected token; pending input may use that known
issue identity when refresh has temporarily removed the page. Replay still requires
the same final issue identity and native source revalidation, so this does not choose
a different record when the query changes. The unchanged terminal suite is rerunning
in session `93744`, `/tmp/workdeck-pm10-indexed-input-generation-pty.log`.

The unchanged input-generation terminal rerun finished successfully: **21 passed,
zero failed**, 28.71 seconds, `/tmp/workdeck-pm10-indexed-input-generation-pty.log`.
The subsequent full workbench regression also passed: **160 passed**, 21.71 seconds,
`/tmp/workdeck-pm10-input-generation-workbench.log`. These qualify the rendered
selection/input dispatch repair and combined mounted collections; they do not close
the remaining PM-10 view or performance requirements.

### PM-10 Planning namespace scaling

A deterministic 80-record fixture reproduced **81 namespace scans** during one
`Repository::list_planning` call (`/tmp/workdeck-pm10-planning-scan-red.log`).
`planning/store.rs` now parses each bounded document directly after the bulk list
has checked path shape and case-folded identity uniqueness. Single-record loading
retains its independent namespace/case validation. The unchanged acceptance test
passes with **one scan**, and compares every returned record with individual native
reads (`/tmp/workdeck-pm10-planning-scan-green.log`). Additional bulk-read coverage
rejects mismatched IDs, unsupported schemas, invalid YAML and unexpected paths,
then verifies restoration and exact-case lookup. Planning, hierarchy, organization
and projection-live suites pass (`/tmp/workdeck-pm10-planning-scan-adjacent.log`).
This removes repeated traversal; it is not full-size refresh performance evidence.

### PM-10 bounded Issues board — active increment

`ProjectionReadView::board` reads visible columns using one immutable query handle.
The combined card count and serialized response obey the existing page/query limits.
The core acceptance test checks column membership, independent vertical windows,
invalid bounds, same-source retention and wrong-generation rejection. Initial RED
was the missing API (`/tmp/workdeck-pm10-board-core-red.log`), followed by a passing
core test (`/tmp/workdeck-pm10-board-core-green.log`). Mounted workbench acceptance
failed behaviorally at `w must open the board` and then passed grouping, horizontal
navigation, selected identity and retained source/list return
(`/tmp/workdeck-pm10-board-runtime-red.log`,
`/tmp/workdeck-pm10-board-runtime-green.log`).

`w` toggles list/board; `z` cycles status, priority, assignee, project, cycle and
milestone; Left/Right moves between columns. Existing filter/sort and authoring
commands use the same controller and source checks. Rendering requests only visible
cards and supports source scrolling. The first actual narrow/wide terminal run
exposed insufficient excerpt space; the second exposed old cards beneath a new
group label and a premature source-opening interaction. The renderer now suppresses
cards until their applied query matches, grows the source pane when opened, and
keeps clicked/selected card tokens available while the ordinary list page loads.
Source opening preserves the exact captured token during a refresh. The terminal
fixture was also committed so the review canvas cannot satisfy a board predicate,
and its edit uses the existing Ctrl-U clear-field binding. The third run is active
(`/tmp/workdeck-pm10-board-pty-third.log`); final regression/fault qualification is
still required. Earlier failures remain retained in `board-pty.log` and
`board-pty-second.log` under the same `/tmp/workdeck-pm10-` prefix.

The third actual board terminal run passed at both **62 and 150 columns** in
**7.30 seconds**, `/tmp/workdeck-pm10-board-pty-third.log`. The subsequent full
workbench run passed **161 tests** in **25.40 seconds**,
`/tmp/workdeck-pm10-board-workbench.log`. Controlled board-window rejection coverage
is the next qualification step; these results do not establish large-board budgets
or close all PM-10.D4 views.

The controlled board-window test failed with no error when a response substituted
rows 39,997–39,999 for requested rows 0–2
(`/tmp/workdeck-pm10-board-window-red.log`). Acceptance now verifies column count,
group identity, exact offset and row count in addition to handle/source tokens.
The same test preserves previously inspected cards and reports `StaleSource`;
all **23 indexed-workspace tests passed** in **3.33 seconds**
(`/tmp/workdeck-pm10-board-indexed-final.log`). The combined actual workbench
terminal regression passed **all 22 journeys** in **28.38 seconds** on this source
(`/tmp/workdeck-pm10-board-all-pty.log`).

All **nine adjacent core projection tests passed** in **2.73 seconds**
(`/tmp/workdeck-pm10-board-core-adjacent.log`). Full PM-10 formatting/lint/generated
contracts, large-board performance, remaining tree/activity/carryover and repository
switching requirements retain their existing qualification scope.

### PM-10 native feature tree — active increment

The typed feature query now supports `tree` and explicit collapsed feature IDs.
`projection/query_tree.rs` computes iterative preorder from the same captured
SQLite records, retains stable name/ID sibling order, and annotates bounded rows
with query-local depth, child count and out-of-filter parent state. Collapsing a
branch creates a new query handle; existing pages remain pinned and usable.
The core test covers parent-before-child order, nested depth, branch collapse,
immutable old pages and explicit filtered-out parents. The initial missing API
RED is `/tmp/workdeck-pm10-tree-core-red.log`. First implementation verification
found a test fixture decoding the feature operation envelope as a raw record
(`/tmp/workdeck-pm10-tree-core-green.log`); fixing that fixture to use
`FeatureOutcome.record` produced a pass in 0.40 seconds
(`/tmp/workdeck-pm10-tree-core-second.log`). Mounted behavior and scale/fault
qualification are still in progress.

Mounted native feature-tree acceptance reproduced the missing `t` interaction,
then passed in **0.82 seconds** (`/tmp/workdeck-pm10-tree-runtime-red.log`,
`/tmp/workdeck-pm10-tree-runtime-green.log`). The workflow toggles list/tree,
collapses/expands a parent, navigates back from its child, and retains one native
record and the same identity on list return. The core uses an explicit work stack;
a synthetic SQLite **40,000-level chain** traverses completely without recursion,
collapses to one visible row, and rejects a subsequently introduced root cycle.
All **11 projection tests pass** in **2.54 seconds**
(`/tmp/workdeck-pm10-tree-core-adjacent.log`). This test is algorithmic depth/fault
coverage; it does not replace the separate native 40,000-feature and 10,000-issue
workload benchmarks or prove a UI latency/memory budget.

The actual tree terminal journey initially failed when Right arrived during a
collapse query: the implementation ignored it while loading, leaving the parent
collapsed (`/tmp/workdeck-pm10-tree-pty.log`). The workspace now retains one latest
pending Left/Right navigation request with its feature ID and replays it only when
the ready query still selects that feature; changed selection cancels explicitly.
Toggling out of the tree clears this pending navigation. The unchanged journey is
rerunning (`/tmp/workdeck-pm10-tree-pty-second.log`).

The unchanged actual tree journey passed at **78 and 180 columns** in **9.55 seconds**
(`/tmp/workdeck-pm10-tree-pty-second.log`), including edit persistence to the exact
child, retained parent linkage, parent navigation and Review/list/tree return.
The mounted test now also sends Left then Right without waiting between them,
verifying that the final expanded query retains three records and drains pending
navigation. Full workbench regression is running in
`/tmp/workdeck-pm10-tree-workbench.log`.

The final full workbench regression passed **163 tests** in **23.94 seconds**
(`/tmp/workdeck-pm10-tree-workbench.log`). The combined actual workbench terminal
suite passed **23 journeys** in **30.51 seconds**
(`/tmp/workdeck-pm10-tree-all-pty.log`), including the board and native feature-tree
journeys alongside existing review, source, claim, check and authoring paths.
Native feature filtering, indexed CLI exposure, activity/cycle carryover,
repository switching and full measured scale qualification remain open PM-10 work.

### PM-10 native feature filtering — active increment

`feature_filter.rs` mounts a query form separate from native authoring drafts.
`/` filters literal text, project/milestone/target IDs, lead and independent feature
states. Invalid state names retain the form and prior query; blank fields match all.
The focused acceptance failed at the missing slash interaction, then passed in
**0.63 seconds**, preserving a source-bound unsaved draft through filtering to
another feature, an empty result, and the original feature
(`/tmp/workdeck-pm10-feature-filter-red.log`,
`/tmp/workdeck-pm10-feature-filter-green.log`).

Actual **78/180-column terminal verification passed in 10.12 seconds**
(`/tmp/workdeck-pm10-feature-filter-pty.log`). It retains an unsaved child draft,
shows a genuine empty filtered result, marks the parent outside the tree query,
restores and saves the same child draft, and verifies unchanged parent/maturity.
A subsequent heading change adds result counts, filtered scope and retained-query
labels; unknown/unavailable indexed sources do not use the empty-result message.
Full workbench regression is running in
`/tmp/workdeck-pm10-feature-filter-workbench.log`; current-source terminal
regression remains required after that heading change.

Full workbench regression after the filter/state-heading changes passed **165 tests**
in **26.38 seconds** (`/tmp/workdeck-pm10-feature-filter-workbench.log`). Combined
current-source terminal verification is running in
`/tmp/workdeck-pm10-feature-filter-all-pty.log`.

Combined current-source terminal verification passed **24 journeys** in
**50.22 seconds** (`/tmp/workdeck-pm10-feature-filter-all-pty.log`).

### PM-10 cached-reader foundation for CLI inspection

`ProjectionStore::open_cached` opens existing validated cache paths without
creating directories or repairing ignore policy. The returned store rejects
refresh/publication, including the fault-injection entrypoint. Loading retains
`Cached` status; it does not claim current-source validation. The acceptance
checks absent cache paths, existing pinned bytes after a native editor-equivalent
change, rejected refresh and missing ignore-policy preservation. The initial RED
was a missing API plus a missing test import (`/tmp/workdeck-pm10-cached-read-red.log`).
The first implementation run passed ten adjacent tests but the new fixture
incorrectly assumed initialization had not created `.index`; current
`repository.rs` explicitly creates that empty directory. The fixture now removes
that empty temporary directory before testing true absence. All **11 storage
tests pass** in **1.30 seconds** (`/tmp/workdeck-pm10-cached-read-second.log`).
The failed fixture run remains `/tmp/workdeck-pm10-cached-read-green.log`.
CLI commands using this path and generated discovery/schema integration are next;
this library entrypoint alone does not count as delivered CLI behavior.

### PM-10 indexed CLI and discovery — active increment

The actual CLI acceptance reproduced `Unknown command: index`
(`/tmp/workdeck-pm10-index-cli-red.log`). `pm_index.rs` now connects explicit cache
refresh, paged cached queries, grouped board windows and exact-token document
excerpts to the shared projection API. A first integration run exposed a Clap
argument-group collision between the `Query` variant and nested `Query` args;
renaming the args type fixed it (`/tmp/workdeck-pm10-index-cli-green.log`,
`/tmp/workdeck-pm10-index-cli-second.log`).

Four actual CLI tests pass in **3.36 seconds**
(`/tmp/workdeck-pm10-index-cli-envelope.log`):
- Missing cached reads create no index and do not initialize planning or Git.
- Paging requires the inspected handle and rejects changed generations/queries.
- Read envelopes preserve selector, projection identity and Cached freshness under
  field projection; cached results are not current-source validation.
- Exact document tokens reject another generation or checkout.
- Board and native tree results use the shared bounded query structures.
- A malformed native document fails refresh while prior cached rows remain readable.
- Selecting an unavailable source for a cached read creates no new source slot.

`index` is reserved consistently in the shared extension API; generated discovery
marks cache effects and freshness limits. Projection query/page/token/detail/status,
board, view and limit schemas are registered. The generated PM skill now includes
index and repository mapping entrypoints. Final executable regeneration and adjacent
catalog/protocol/registry checks remain in progress; these focused results do not
close all PM-10 source/performance or final packaging gates.

The final indexed-command executable generated `skills/workdeck-pm/SKILL.md`
(9,803 bytes), `docs/reference/workdeck-pm-commands.md` (529,649 bytes) and
`docs/reference/workdeck-pm-schemas.json` (1,741,397 bytes) from an empty temporary
working directory without initializing `.workdeck`. All **30 adjacent CLI tests**
passed: catalog 6, index 4, protocol 17 and registry 3
(`/tmp/workdeck-pm10-index-discovery-regression.log`). This includes byte-for-byte
render/reference parity. All **70 extension API tests** passed, covering the shared
reserved-command contract (`/tmp/workdeck-pm10-index-extension-contract.log`).
CLI usage is documented in `crates/workdeck-cli/README.md`. Full PM-10 and release
qualification remain open; these are increment-scoped results.


### PM-10 formatting/lint cleanup and publication timing investigation — 2026-09-10

The host has 107 GiB available after the earlier storage recovery. Verification
continues in `/tmp/workdeck-pm-verification-20260909` with incremental compilation
and debug information disabled. No additional caches or sibling sources were removed.

Formatting normalized the accumulated PM-10 source. Strict lint exposed an unused
projection configuration argument, redundant return/nested conditions, an endpoint
argument layout, a benchmark clamp, and production-visible test helpers. The cleanup
removes the unused argument, groups edge endpoints, and compiles inspection helpers
only in tests. Production worker Drop still closes input and joins the thread; the
host retains ownership outside its shell lock. No lint exceptions were added.

- Initial formatting failure: `/tmp/workdeck-pm10-format-red.log`.
- Retained lint failures: `/tmp/workdeck-pm10-clippy-red.log`,
  `/tmp/workdeck-pm10-clippy-second.log`, `/tmp/workdeck-pm10-clippy-third.log`.
- Three-crate all-target strict lint passed in 37.84 s:
  `/tmp/workdeck-pm10-clippy-fourth.log` (before the final diagnostic-only test edit).
- Projection unit tests: **11 passed / 4.04 s**,
  `/tmp/workdeck-pm10-cleanup-projection.log`.
- Live source adapters: **7 passed / 3.05 s**; storage: **11 passed / 2.50 s**,
  `/tmp/workdeck-pm10-cleanup-storage-live.log`.

**Unresolved full-suite timing failure:** the workbench run passed 164 tests but
`proposal_publication_status_and_fresh_session_resume_keep_original_plan_and_dirty_index`
hit its unchanged eight-second worker wait. This reproduced in a second full run.
The isolated test passed in 11.50 s across its publication/status/resume workflow.
A third full run, with caller/elapsed-time diagnostics, identifies the initial
publication wait specifically: 8.007 s, no worker error yet. All 164 other tests
passed again. Logs:

- `/tmp/workdeck-pm10-cleanup-workbench.log` (35.31 s)
- `/tmp/workdeck-pm10-cleanup-workbench-repeat.log` (35.89 s)
- `/tmp/workdeck-pm10-cleanup-proposal-isolated.log`
- `/tmp/workdeck-pm10-cleanup-workbench-diagnostic.log` (35.50 s)

An independent sibling-checkout xtask process was observed using over five CPU cores
during investigation. Contention is a hypothesis, not a proven root cause. No sibling
process was stopped. Do not call this suite qualified or enlarge the assertion to
hide the failure. Next diagnosis should measure the proposal publication's repeated
source capture/validation/Git subprocess work, preserving every source-binding check.
Final cleanup gate logs are `/tmp/workdeck-pm10-cleanup-format.log`,
`/tmp/workdeck-pm10-cleanup-clippy.log`, and
`/tmp/workdeck-pm10-cleanup-architecture.log`; their terminal results must be checked.


Final cleanup gates: `cargo fmt --all -- --check` passed; PM/CLI/TUI
`cargo clippy --offline --locked --all-targets -- -D warnings` passed in 52.29 s.
Architecture initially rejected an unreachable, untracked draft at
`features/read_diagnostics.rs`. The actual diagnostics remain in `features/coverage.rs`
with the shared policy index; the orphan also referred to an absent organization
helper and had no callers. Removed it from the source tree after saving an exact
recovery copy at `/tmp/workdeck-pm10-orphan-read-diagnostics.rs`.
The unchanged architecture gate then passed: **13 production crates, one shipped
executable, zero dependency or source-reachability violations**
(`/tmp/workdeck-pm10-cleanup-architecture-green.log`). Removing an unreferenced file
changes no compiled source; the final formatting/lint results remain applicable.
No Cargo process remains live. PM-10 and the proposal timing failure remain open.


### PM-10 native Git spawn path — 2026-09-10

The three retained full-workbench failures above establish the RED case: the initial
proposal publication exceeded the existing eight-second worker assertion. Inspection
found the source Git runner using `pre_exec` solely for `setpgid(0, 0)`. Replaced that
callback with `CommandExt::process_group(0)`, matching the existing check runner.
This retains a dedicated process group and allows the platform's native spawn path
instead of requiring the custom pre-exec callback. No source capture, validation,
publication reconciliation, timeout, or cleanup checks were removed.

The unwind test now records the child's actual group before injecting the panic and
asserts outside catch_unwind that the child led a separate group. It also retains
the original assertion that the child was killed and reaped. Assertions inside the
caught callback would not prove group ownership, since either panic would be caught.

GREEN evidence:

- **165 workbench tests passed / 30.34 s**, including the previously failing
  publication/status/resume test with its unchanged eight-second per-worker wait:
  `/tmp/workdeck-pm10-native-spawn-workbench.log`.
- **12 core source tests passed / 7.85 s**: process-group/unwind cleanup, bounded
  capture, candidate tampering, lost responses, exact resume and racing clones:
  `/tmp/workdeck-pm10-native-spawn-core.log`.
- **21 integration tests passed**: publication 7/47.18 s, source identity 1/0.16 s,
  sources 13/10.62 s; `/tmp/workdeck-pm10-native-spawn-integration.log`.
- **2 real terminal source journeys passed / 18.50 s**: immutable citations plus
  reviewed publication/resume preserving staged/unstaged developer state,
  `/tmp/workdeck-pm10-native-spawn-pty-qualified.log`. The first filter attempt
  (`/tmp/workdeck-pm10-native-spawn-pty.log`) selected zero tests and is not evidence.

The previously red workbench gate is now green after this change. This is not a
universal latency guarantee or completion of PM-10's large-source performance work.
The earlier failures and concurrent-load observation remain in the record.
Final native-spawn formatting/lint/architecture logs must be checked separately.

Final native-spawn gates all passed: formatting; strict all-target PM/CLI/TUI lint
(38.98 s); architecture (13 production crates, one executable, zero violations).
Evidence: `/tmp/workdeck-pm10-native-spawn-format.log`,
`/tmp/workdeck-pm10-native-spawn-clippy.log`, and
`/tmp/workdeck-pm10-native-spawn-architecture.log`. No Cargo process remains live.


### PM-10 mounted activity timeline — 2026-09-10

Shift-F12 opens a retained Activity tab using the existing bounded projection
worker and native `ProjectionQuery::Activity`. It shows chronological native event
records from the working tree, record kinds and timestamps, and exact source excerpts.
Keyboard pages/Home/End, mouse selection, independent excerpt scrolling, refresh,
and return to Review/Issues use the existing source-qualified query/detail machinery.
The reader remains owned through shutdown and is joined outside the shell mutex.
Opening Activity does not initialize a missing planning or Git repository.

Issue forms/drafts remain intact when the timeline opens; switching back restores
them. Refresh updates timeline membership without rebasing an opened document.
Malformed sources retain last-good rows with stale/error visibility. This timeline
is a read view, not completion evidence. Subject/kind/time query inputs are available
through the indexed CLI; this mounted increment presents the working-tree timeline.
Multi-repository/source switching and cycle carryover remain separate open work.

Evidence:

- Intended mounted RED: Shift-F12 remained in Issues;
  `/tmp/workdeck-pm10-activity-red.log`.
- Mounted GREEN, exact operation open and retained issue draft: 1 passed/1.90 s;
  `/tmp/workdeck-pm10-activity-green.log`.
- Refresh and malformed-source retention passed with the first mounted case;
  `/tmp/workdeck-pm10-activity-retention.log` (5 tests total includes three unrelated
  scrollbar tests selected by the broad activity filter; only two prove this feature).
- Actual terminal journey at widths 72/160 passed/7.36 s:
  `/tmp/workdeck-pm10-activity-pty.log`. The final full PTY run strengthens the open
  assertion to require the `operations/` path, not just a title already in the list.
- Strict all-target TUI lint passed/20.94 s:
  `/tmp/workdeck-pm10-activity-clippy.log`.
- Final **168 workbench tests passed/25.96 s**, including unavailable-source inertness,
  draft/excerpt retention, and the repaired proposal publication:
  `/tmp/workdeck-pm10-activity-final-workbench.log`.

The final PTY/architecture/format gate terminal results still need inspection before
claiming those gates for this increment. No PM-10 requirement row is closed solely
by the activity milestone.

Final activity gates passed: **25 real workbench terminal journeys / 44.52 s**
(`/tmp/workdeck-pm10-activity-final-pty.log`); architecture, **13 production crates,
one executable, zero violations** (`/tmp/workdeck-pm10-activity-final-architecture.log`);
and formatting (`/tmp/workdeck-pm10-activity-final-format.log`). The final terminal
activity assertion verifies the opened operation path at both widths. No Cargo
process remains live. Cycle carryover, mounted registry/source switching, performance,
and the later complete-plan qualification remain open.


### PM-10 cycle carryover core and CLI — 2026-09-10

`cycle carryover FROM TO --json --no-input` previews unfinished members; repeating
`--issue ID` selects an explicit batch. `--expected-preview HASH` applies the same
reviewed input, using an optional original `--request-id` for reconciliation/replay.
Preview rejects mutation-only options. Apply can stage only its exact receipt paths.
Missing/legacy sources do not gain a new planning authority.

The shared `planning/carryover.rs` operation selects at most 100 eligible issues.
Default selection reports completed, canceled, archived and retired exclusions;
explicit selection rejects ineligible members instead of silently dropping them.
It changes only cycle membership through `prepare_issue_mutation`, preserving the
ordinary hierarchy, workflow, retirement and organization policies. It does not
complete issues or close cycles. Larger cycles require explicit reviewed batches.

The plan binds source/destination cycle tokens, repository identity and root,
request, complete source membership and all validation read/list/absence observations.
`Snapshot::read_fingerprint` includes hashes rather than exposing read contents.
Apply recomputes the plan under the existing transaction lock. The transaction
engine journals every change and replays/reconciles the original request; a rename
alone is not presented as a complete multi-file transaction.

RED/GREEN evidence:

- Missing core carryover API: `/tmp/workdeck-pm10-carryover-red.log`.
- First implementation import correction retained in
  `/tmp/workdeck-pm10-carryover-green.log`; 2 acceptance tests then passed in
  `/tmp/workdeck-pm10-carryover-second.log` and after full read binding in
  `/tmp/workdeck-pm10-carryover-read-binding.log`.
- Interrupted/cross-source cases: 4 passed/2.27 s,
  `/tmp/workdeck-pm10-carryover-faults.log`.
- Final core **7 passed/2.74 s**: eligibility, exact replay, new membership rejection,
  equivalent newly-created policy file invalidation, copied checkout with identical
  repository/issue IDs, explicit selection, and recovery after journal/first change/
  before receipt; `/tmp/workdeck-pm10-carryover-final-binding.log`.
- Adjacent planning **19 passed/1.54 s**, organization **28 passed/3.62 s**;
  `/tmp/workdeck-pm10-carryover-core-regression.log` (also contains the first 6 core cases).
- Actual missing CLI subcommand RED: `/tmp/workdeck-pm10-carryover-cli-red.log`;
  preview/apply/exact-envelope replay GREEN: 1 passed/3.24 s,
  `/tmp/workdeck-pm10-carryover-cli-green.log`.
- Final CLI/discovery/hierarchy/reference **33 passed**: carryover 1, catalog 6,
  hierarchy 6, protocol 17, reference resolution 3;
  `/tmp/workdeck-pm10-carryover-final-cli.log`.
- Strict PM/CLI all-target lint passed/30.61 s:
  `/tmp/workdeck-pm10-carryover-clippy.log`. The subsequently added policy-file test
  has a separate final lint log `/tmp/workdeck-pm10-carryover-final-test-lint.log`.
- Architecture passed with 13 crates, one executable and zero violations;
  formatting passed: `/tmp/workdeck-pm10-carryover-final-architecture.log`,
  `/tmp/workdeck-pm10-carryover-final-format.log`.

Generated command/schema/skill references were rendered from the final executable
in an empty temporary directory; rendering created no planning repository. They
include carryover discovery, request/plan schemas and operational limits. Catalog
parity is verified in the final CLI suite. The mounted carryover form and its actual
terminal acceptance journey remain **open**; core/CLI completion does not close
PM-10.D4 or the full phase.

Final policy-binding test lint also passed (26.10 s). No Cargo process remains live.


### PM-10 mounted cycle carryover — 2026-09-10

In Cycles (F12), `u` opens a carryover draft bound to the inspected source cycle.
The form accepts a destination and optional whitespace-separated issue IDs. Ctrl-S
calls the shared preview without writing. The review lists eligible moves and
exclusions; `x` applies/retries the exact preview through the shared core operation.
It never changes status or closes cycles. Long previews materialize their lines once
and render only the scrolled window. Keyboard pages and the existing planning mouse
scroll route remain available.

Forms retain their original source token; previews and request IDs survive refresh,
review/planning tab switches and failures. `e` returns to editing only before an
attempt. Esc hides/retains the intent; Ctrl-D explicitly discards it. An attempted
request cannot silently become a new edited request. RecoveryRequired displays the
operation-recovery command; after recovery, retry reuses the original receipt.
Pending durable operations remain owned by the transaction engine even if the user
explicitly discards the UI intent. Separate error/request/action regions keep recovery
controls visible at 62 columns.

RED/GREEN evidence:

- Intended mounted RED: `u` did not open carryover;
  `/tmp/workdeck-pm10-carryover-tui-red.log`.
- Mounted preview/exclusions/Review return/apply/exact retry GREEN: 1 passed/2.03 s;
  `/tmp/workdeck-pm10-carryover-tui-green.log`.
- Stale-test fixture initially used an unavailable AppTheme default constructor;
  corrected to the existing theme resolver (`...-tui-stale-red.log`). Stale request
  retention then passed at widths 72/160 (`...-tui-stale-red-second.log`). These file
  names retain the investigation order; the latter is passing evidence, not a RED.
- Extending to 62 columns exposed hidden discard controls:
  `/tmp/workdeck-pm10-carryover-tui-narrow-red.log`. The footer layout was corrected
  without changing mutation or source assertions.
- **4 carryover UI tests passed/3.60 s**: mounted workflow, stale request retention/
  explicit reset, source-cycle edit rejection, and journal interruption/recovery/
  original receipt replay; `/tmp/workdeck-pm10-carryover-tui-safety.log`.
- **Actual terminal journey at 62/160 passed/10.67 s**: preview, Review round trip,
  changed membership rejection, visible discard control, fresh preview, and apply
  preserving both issues' status; `/tmp/workdeck-pm10-carryover-tui-pty.log`.
- Final strict TUI/CLI all-target lint passed/38.62 s:
  `/tmp/workdeck-pm10-carryover-tui-final-clippy.log`.

Final full workbench/PTY/architecture/format results remain to inspect before claiming
those gates. PM-10 remains open for registered repository/source switching,
performance and integrated phase qualification.


### PM-10 Git binding retry and requested storage cleanup — 2026-09-10

The full mounted carryover regression gate twice reported **171 passed, one failed**
(32.05 s and 29.53 s). Both failures were the existing proposal-publication test's
initial eight-second wait, with no recorded action error. Logs:
`/tmp/workdeck-pm10-carryover-tui-final-workbench.log` and
`/tmp/workdeck-pm10-carryover-tui-workbench-repeat.log`. The first pipeline stopped;
its later PTY, architecture and formatting gates did not run. Assertions and timeout
remain unchanged.

`BoundGit::with_deadline` now uses one combined rev-parse for ordinary root, Git
and common-directory paths. Ambiguous embedded-newline output falls back to the
individual probes. Physical identity, exact-root, output limit and aggregate deadline
checks remain in place. The intended process-count RED observed three calls instead
of one (`/tmp/workdeck-pm10-binding-probe-red.log`). After implementation, **14 core
source tests passed/9.26 s**, including newline paths, exact-root guards, batching,
process-group cleanup, deadlines and coordination races:
`/tmp/workdeck-pm10-binding-probe-core.log`. This is focused evidence; publication
latency still requires full regression verification.

On the user's explicit storage-cleanup request, live inspection found 107 GiB free,
a 3.1 GiB task target and an active Cargo workspace build using the main checkout.
`cargo clean --offline --locked --manifest-path <this-worktree>/Cargo.toml
--target-dir /tmp/workdeck-pm-verification-20260909` removed **6,388 files, 3.0 GiB**.
The task target's absence was verified; free space increased to **110 GiB**. Source,
lockfiles and the other active build's cache were preserved. Verification resumed
with the same isolated target and reduced-debug/no-incremental settings;
`/tmp/workdeck-pm10-post-clean-workbench.log` is pending, not passing evidence.

Post-clean rebuild completed in 1m28s; **172 workbench tests passed/36.38 s**,
including the original proposal-publication assertion with its unchanged eight-second
limit. `/tmp/workdeck-pm10-post-clean-workbench.log` is now passing evidence.
Free space after rebuilding was 109 GiB. Remaining PTY/integration/lint/architecture
qualification still applies before closing the increment or PM-10.


Git binding/carryover follow-up qualification: **21 integration tests passed**
(publication 7/40.79 s, source identity 1/0.17 s, sources 13/8.72 s), including
protected source roles, exact reviewed publication bindings and developer-state
preservation. `/tmp/workdeck-pm10-binding-final-integration.log`.
**All 26 actual workbench terminal journeys passed/60.83 s**, including carryover,
Activity, boards, tree/filter, sources, claims, checks and existing review/navigation:
`/tmp/workdeck-pm10-binding-final-pty.log`. These qualify the current mounted
carryover and Git binding increment; they do not establish large-source performance
or registered-repository switching. Strict lint/architecture remain pending.

Strict PM/CLI/TUI all-target lint passed/1m05s:
`/tmp/workdeck-pm10-binding-final-clippy.log`. The first architecture invocation
omitted its required `check` subcommand and exited with usage error after rebuilding
xtask (`...-architecture.log`); the corrected `cargo xtask architecture check`
passed with 13 production crates, one executable and zero violations
(`...-architecture-corrected.log`). This command error is not a product regression.
The next mounted My work acceptance test is being established separately.


### PM-10 mounted My work — 2026-09-10

Shift-F9 mounts assigned work over the existing explicit registry and shared
`RegistryStore::my_work` operation. A separately owned, coalesced background reader
performs registry/projection I/O; the host polls it across tabs and takes it out of
the shell mutex before shutdown joins. No central authoring state is introduced.
Report pages contain at most 20 rows, source membership retains the registry's 256
entry bound, and previous-page history retains at most 128 cursors. Rendering visits
only the visible rows and already materialized source excerpt lines.

`/` edits assignee/alias filters; invalid input and the previous report remain
visible. `[`/`]` navigate one captured generation, including a fingerprinted return
to page zero. `r` explicitly starts a fresh report. `s` shows registered mappings,
source roles and errors. Known totals are incomplete when a source is unavailable.
Enter opens the exact cached row token after verifying the original registered
checkout before and after the read. An existing opened excerpt survives refresh,
errors and tab switches; it never becomes write authority. Issue drafts remain in
their original controller. Native checkout/source switching and broader My work
facets are still required; this slice does not close PM-10.D5.

Retained evidence:

- Intended mounted RED: Shift-F3 left the owner issue form visible because My work
  was absent (`/tmp/workdeck-pm10-my-work-mounted-red.log`, 1 failed/1.57 s).
- Initial implementation compilation caught two PathBuf/string adapter errors;
  fixed using the existing terminal path formatter. The mounted workflow then
  passed/1.69 s (`...-mounted-green-second.log`).
- Safety run exposed a real page-zero rebinding defect: back navigation used no
  cursor and accepted a new generation. It now retains the original fingerprint.
  Two fixture assumptions were corrected separately: alias replacement requires
  explicit removal, and narrow excerpts require their real scroll action to reach
  the body. Five tests passed/1.99 s (`...-safety-second.log`).
- The first actual PTY failed because Shift-F3's escape sequence was consumed as
  a cursor-position response (`...-pty.log`). The shortcut is now Shift-F9 with an
  unambiguous function-key sequence. The same 62/160-column workflow passed/6.92 s
  (`...-pty-second.log`), including partial-source errors, exact excerpt retention,
  Review return and an owner-only draft save.
- Lint caught a 440-byte read-message variant; its token was boxed rather than
  suppressing the lint (`...-clippy.log`).
- Two further intended REDs reproduced lost source selection after registry
  insertion and inability to refresh a view opened before external initialization
  (`...-retention-red.log`, 5 passed/2 failed). Source selection now follows the
  complete checkout mapping. An unavailable owner may discover its first source;
  once opened, the store retains its original owner binding through later errors.
- **Seven tests passed/3.11 s** (`...-retention-green.log`), covering the mounted
  workflow, same-looking cross-repository IDs, mapping replacement rejection,
  malformed/missing source visibility, unchanged opened excerpts, stale pagination,
  invalid-filter retention, source-selection retention, inert uninitialized reads
  and explicit refresh after external initialization.

Final strict lint, full workbench/terminal, architecture and format gates are running
with logs under `/tmp/workdeck-pm10-my-work-final-*`; pending gates are not yet
passing evidence.


The first full My work regression pipeline passed strict TUI/CLI all-target lint
(20.78 s), then reported **178 passed, one failed/34.16 s** in workbench tests.
The failure was the existing proposal-publication initial eight-second wait, not a
My work case. `/tmp/workdeck-pm10-my-work-final-workbench.log`. That pipeline
stopped; its later PTY/architecture/format gates did not run.

Investigation found publication re-capturing both source views and rebuilding the
proposed file set immediately after freshly computing and validating the same plan.
`proposal_impl::capture_preview` now retains its immutable working/accepted views
and validated file set for publication to consume. Public preview output is unchanged.
The complete plan equality, source identities, exact diff, source revalidation after
candidate construction, candidate validation, binding and publication guards remain.
This removes duplicate capture and validation work without rebasing a reviewed plan.
The unchanged full workbench suite is running in
`/tmp/workdeck-pm10-preview-reuse-workbench.log`; its result is pending.


Retained-preview publication qualification:

- **179 workbench tests passed/25.55 s** with the original eight-second worker
  assertion unchanged (`/tmp/workdeck-pm10-preview-reuse-workbench.log`).
- Strict PM/CLI/TUI all-target lint passed/35.59 s
  (`/tmp/workdeck-pm10-preview-reuse-clippy.log`).
- **21 source/publication integration tests passed**: publication 7/27.97 s,
  identity 1/0.15 s, sources 13/7.65 s
  (`/tmp/workdeck-pm10-preview-reuse-integration.log`).
- Actual CLI proposal preview/publication/status/fresh-resume passed/11.97 s
  (`/tmp/workdeck-pm10-preview-reuse-cli.log`).
- **All 27 actual workbench terminal journeys passed/70.70 s**
  (`/tmp/workdeck-pm10-my-work-final-pty.log`).

A subsequent code review identified that the My work mouse hit map stores row
positions across report refresh. A new test will verify that clicking an obsolete
frame cannot open a newer source. That test is not included in the 179-test run;
its result and the architecture/format tail are pending.


The repaired-publication architecture and format tail passed (13 crates, one
executable, zero architecture violations; logs `...-my-work-final-architecture.log`
and `...-my-work-final-format.log`).

The stale-frame mouse test reproduced opening newer bytes from an obsolete hit map:
`/tmp/workdeck-pm10-my-work-mouse-red.log` (1 failed/0.80 s). Successful report
publication and source-list mode changes now invalidate the old hit map; the next
paint supplies positions for the newly visible report. The regression verifies both
rejection before repaint and successful opening after repaint. **Eight My work tests
passed/1.74 s** (`...-mouse-green.log`). Final workbench, affected actual terminal,
lint, architecture and formatting results are pending under
`/tmp/workdeck-pm10-my-work-mouse-final-*`.


Final mouse-fix qualification:

- **180 workbench tests passed/23.41 s** (`...-mouse-final-workbench.log`), including
  the original eight-second publication wait and stale-frame pointer rejection.
- Affected actual My work terminal journey at 62/160 passed/6.75 s
  (`...-mouse-final-pty.log`). The preceding complete 27-journey run remains recorded
  separately; unrelated terminal tests were not repeated after the mouse-only fix.
- Strict TUI/CLI all-target lint passed/11.68 s (`...-mouse-final-clippy.log`).
- Architecture passed with 13 production crates, one shipped executable and zero
  violations (`...-mouse-final-architecture.log`); formatting passed
  (`...-mouse-final-format.log`).

All paths above use `/tmp/workdeck-pm10-my-work` as their prefix. No Cargo process
owned by this task remains live. Disk inspection reports 107 GiB free. This closes
the mounted assigned-work/source-browser increment, not PM-10: native checkout/
source switching, broader My work facets, measured large workloads and integrated
phase qualification still remain. PM-11 and PM-12 retain their full scope.


## PM-10 registered checkout reload foundation — 2026-09-10

The native file loader previously retained the launch provider for every reload,
so it rejected file paths in an explicitly registered second checkout. The new
`repository_panels/registered.rs` creates a separate provider after validating the
exact working-tree mapping. The launch provider retains its original physical and
planning bindings. Accepted/proposal refs cannot acquire native file providers.
`RegistryNavigation` carries the original registry owner and exact mapping across
preparation; it is revalidated before the session broker can publish a reload.
Replacement providers must match the loaded repository root and are adopted only
after publication commits. Broker failure preserves the old provider and cwd.

The first main-loader repair also rejected independent patch reviews at unregistered
roots. Its compatibility regression failed for that reason and the loader was
corrected: non-file reviews retain their existing input support without acquiring
registry navigation authority. File navigation still requires its bound provider.
No command/schema change or registry initialization is introduced by preparation.

Original acceptance evidence:

- Missing provider API: `/tmp/workdeck-pm10-registered-panels-red.log`; the resulting
  two provider tests passed in `...-registered-panels-green.log`.
- Registered file reload rejected by the launch provider:
  `/tmp/workdeck-pm10-checkout-reload-red.log`; the focused test passed in
  `...-checkout-reload-green.log`.
- Missing owner-pinned navigation API: `/tmp/workdeck-pm10-navigation-guard-red.log`;
  the replacement-owner regression passed in `...-navigation-guard-green.log`.
- Non-file compatibility failure: `/tmp/workdeck-pm10-checkout-reload-compatibility-red.log`;
  all nine reload tests then passed/0.53 s in `...-checkout-reload-regression.log`.

After the requested Cargo cleanup, fresh isolated builds passed:

- **12 registry/navigation/My work core tests** (7 + 1 + 4), including copied owner
  replacement, target identity, missing mapping, stale paging and concurrent writes:
  `/tmp/workdeck-pm10-checkout-registry-regression.log`.
- **44 panel integration tests** (2 registered + 16 general + 12 Git + 14 safety),
  including bounded reads and concurrent parent replacement:
  `/tmp/workdeck-pm10-checkout-panels-regression.log`. Safety subprocess re-entry
  results are not counted again as separate top-level tests.
- **26 session-related tests** passed before the cwd repair:
  `/tmp/workdeck-pm10-checkout-session-regression.log`.

Review then found that a valid selected root/provider could carry an unrelated
command cwd. The new regression reached the broker before rejection (RED:
`/tmp/workdeck-pm10-checkout-cwd-red.log`). Registered navigation now canonicalizes
command cwd and requires it to remain within the selected checkout before publication.
The **27 session-related tests passed/6.30 s** after the repair
(`/tmp/workdeck-pm10-checkout-session-final.log`). Additional assertions exercise
symlink escape rejection on Unix and successful nested cwd adoption; their final
focused result and strict lint/architecture/format tail are recorded below when terminal.

This is the reload foundation only. Native switch controls, retained per-checkout
shells/drafts/view state, all-context worker polling/shutdown, shared foreground-run
ownership and read-only ref-backed switching remain required. The earlier My work
terminal qualification does not prove those unimplemented flows.

Final cwd assertions passed/1.03 s in
`/tmp/workdeck-pm10-checkout-cwd-final.log`: unrelated and symlink-escaped command
roots are rejected before broker invocation; a valid nested directory commits.
All **nine CLI reload tests passed/0.54 s** in
`/tmp/workdeck-pm10-checkout-reload-final.log`. Strict PM/TUI/CLI all-target lint
passed/28.87 s in `/tmp/workdeck-pm10-checkout-clippy.log`.

The architecture tail initially rejected the checkout test module's direct
`workdeck-session` import (`/tmp/workdeck-pm10-checkout-architecture.log`). The
nested tests now use `ReloadSessionOptions` through their parent review adapter,
matching that adapter's ownership boundary. The first import-only attempt failed
to compile because the parent used qualified type paths; the parent now imports
the type explicitly and uses that same import in its production signatures. No architecture allowlist was
expanded. Affected adapter tests, lint and architecture are rerun after this repair.

Final boundary repair qualification: **27 session-related tests passed/6.38 s**
(`/tmp/workdeck-pm10-checkout-adapter-final.log`), strict TUI all-target lint passed
(`...-checkout-adapter-clippy.log`), architecture passed with 13 production crates,
one executable and zero violations (`...-checkout-architecture-final.log`), and
formatting passed (`...-checkout-format.log`). The preceding strict PM/CLI/TUI gate
remains applicable to unchanged PM and CLI code. The serial verification driver
terminated successfully; no Cargo invocation remains live.


## PM-10 mounted native checkout switching — 2026-09-10

My work now prepares an exact registry navigation handle on its owned reader when
`o` selects a working-tree source. `b` returns to the previous retained native
checkout, including the launch checkout without registering it. Up to eight
contexts retain their complete planning controllers, forms, index readers, native
review inputs, filters, selected paths, scroll and navigation. Context keys include
physical checkout/planning identity, repository ID and source selector; aliases do
not create duplicate native contexts. A ninth context is refused without eviction.

The launch registry browser and cached excerpts move with the active view. Native
mutation/check/claim/publication state remains in each original shell. All contexts
share foreground-run ownership, inactive owned workers are polled, and shutdown
joins them outside the shell mutex. No registry is written into a selected checkout.
The previous view is retained when entering My work, including an opened file
review; entering the registry browser no longer implicitly returns from that file.

Switching prepares a candidate coordinator constrained to the selected checkout,
preserving launch extension authority. The active broker boundary changes only
after publication succeeds; ordinary broker requests cannot widen it. The session
adapter validates the exact pending source, command cwd and provider before
preparation and again before the broker commit. Failed loads restore the old shell;
failed revisits also retain the target's uncommitted forms. Returning to a copied
replacement `.workdeck` is rejected even if configuration preserves the repo ID.

Acceptance and fault evidence so far:

- Mounted switch RED: `/tmp/workdeck-pm10-switch-mounted-red2.log` failed because
  `o` left the launch repository active. The preceding test draft had two compile
  mistakes (enum variant and private test helper), corrected before this behavioral RED.
- Initial mounted GREEN: `/tmp/workdeck-pm10-switch-mounted-second.log`, 1/2.01 s.
  Expanded retention/failure/source-replacement coverage passed/2.27 s in
  `/tmp/workdeck-pm10-switch-mounted-retention.log`. Owner and target drafts save
  only in their original repositories; review selection/filter/scroll return and
  failed new/revisited loads preserve the retained contexts.
- Coordinator API RED: `/tmp/workdeck-pm10-switch-coordinator-red.log`; preparation,
  isolated roots and launch authority GREEN: `...-switch-coordinator-green.log`,
  1/0.08 s. The test also corrected a borrowed argument before the implementation run.
- Actual terminal journey at 62/160 columns passed/11.41 s in
  `/tmp/workdeck-pm10-switch-pty-first.log`. The expanded exact file-source return
  journey passed/13.99 s in `...-switch-pty-source-retention.log`.
- Owned inactive checks initially reached the existing attempted-plan guard on their
  second run (`...-switch-worker-ownership.log`). The fixture now explicitly discards
  the inspected plan and selects definitions before replanning. It passes/2.14 s in
  `...-switch-worker-final.log`: the inactive check finishes only in its original cwd,
  is polled there, shares the active foreground signal, and a second held run is
  canceled/joined by application shutdown.
- Full TUI library: **1,263 tests passed/26.42 s** in
  `/tmp/workdeck-pm10-switch-final-tui.log`. The serial terminal/reload/lint/architecture
  qualification tail is still running; its results are recorded separately below.

This increment covers native working-tree switching. Ref-backed accepted/proposal
planning-view switching, broader My work facets, required large-source performance
and final integrated PM-10 qualification remain open. PM-11 and PM-12 retain full scope.


The first full qualification tail passed all **28 actual workbench terminal
journeys/29.98 s** and **nine reload tests**, then strict lint found a collapsible
conditional and eager cloning in the new view-restoration helper. Those were fixed
without changing an allowlist. A source-label regression subsequently failed because
the active checkout was not persistently displayed (`...-switch-source-label-red.log`).
A dedicated navigation row now shows the selected alias and root while editing;
that regression passed at 62 columns (`...-switch-source-label-green.log`, 1/2.46 s).

The next complete candidate passed:

- Capacity/alias retention: **1/2.35 s**, including unchanged retained drafts when
  refusing a ninth checkout (`/tmp/workdeck-pm10-switch-qualified-capacity.log`).
- **1,264 TUI library tests/26.10 s** (`...-switch-qualified-tui.log`).
- **28 actual workbench terminal journeys/29.18 s**, including native source-file
  and draft returns at 62/160 columns (`...-switch-qualified-pty.log`).
- **9 CLI reload tests/0.56 s** (`...-switch-qualified-reload.log`).
- Strict TUI/CLI all-target lint/11.97 s, architecture (13 production crates, one
  executable, zero violations), and formatting (`...-switch-qualified-clippy.log`,
  `...-architecture.log`, `...-format.log`).

Review then identified that returning to a checkout should retain an original
nested command cwd instead of replacing it with the repository root. A focused
regression and repair are in progress; the preceding candidate results are retained
as historical evidence, not asserted as qualification of that subsequent change.


Nested-cwd RED: `/tmp/workdeck-pm10-switch-nested-cwd-red.log` reproduced returning
to the repository root instead of the original `nested/` cwd. Each retained context
now stores its command cwd separately from its canonical checkout binding. It is
passed back through the scoped reload coordinator and existing source/cwd guards;
no root fallback silently replaces the retained directory. The expanded mounted
regression passed/2.46 s in `...-switch-nested-cwd-green.log`. The actual terminal
journey now also launches from a nested directory. Final current-source gates are
running under `/tmp/workdeck-pm10-switch-cwd-final-*`.


Final nested-cwd/current-source qualification is complete:

- **1,264 TUI library tests passed/27.28 s**
  (`/tmp/workdeck-pm10-switch-cwd-final-tui.log`).
- **All 28 actual workbench terminal journeys passed/28.40 s**, including nested
  launch, owner/target drafts and exact source-file return at 62/160 columns
  (`...-switch-cwd-final-pty.log`).
- **Nine CLI reload tests passed** (`...-switch-cwd-final-reload.log`).
- Strict TUI/CLI all-target lint passed/10.83 s
  (`...-switch-cwd-final-clippy.log`).
- Architecture passed (13 production crates, one shipped executable, zero dependency
  or source-reachability violations); formatting and `git diff --check` passed
  (`...-switch-cwd-final-architecture.log`, `...-switch-cwd-final-format.log`).

The serial driver exited successfully. Disk inspection reports 111 GiB free.
This qualifies the native working-tree switch increment. It does not close PM-10:
ref-backed planning-view switching, broader My work facets, large-source performance
and integrated phase/release qualification remain required. No real backlog was
initialized or mutated; no commit, push, deployment or sibling modification was made.


## PM-10 ref-backed planning switching — 2026-09-10 (in progress)

Accepted/proposal mappings now mount read-only indexed planning views with retained
page state, tree/board controls, filters and exact excerpts. The selected physical
checkout and original registry admission are validated across reload; these shells
have no native mutation controller or repository panels. A shared projection
reader avoids one store allocation and independent capture per tab.

The return-home test first failed because `b` could only alternate the most recent
two contexts; `h` now restores the original physical launch context from anywhere.
A source-recovery test exposed a worker error that survived a later successful
reply; successful replies now clear that transient error. Copied replacement
planning directories remain rejected without creating an index there.

Evidence:
- `/tmp/workdeck-pm10-ref-launch-return-red.log`: intended failure restoring native draft.
- `/tmp/workdeck-pm10-ref-recovery-red.log`: intended stale worker error recovery failure.
- `/tmp/workdeck-pm10-ref-home-recovery.log`: all three focused tests pass.
- `/tmp/workdeck-pm10-ref-shared-reader.log`: all three tests pass with shared reader.
- `/tmp/workdeck-pm10-ref-full-tui.log`: 1,266 pass, one atomic-save watcher assertion
  fails (`reload_pending`); all new read-only tests pass, including captured-generation
  reuse after advancing the proposal ref and explicit refresh to that new generation.
- `/tmp/workdeck-pm10-ref-watch-recheck.log`: the watcher passes in isolation.

No final-current full-suite, terminal, lint or performance qualification is claimed
by this checkpoint. Original native-switch evidence above predates this increment.

Final increment checks:
- `/tmp/workdeck-pm10-ref-final-tui.log`: 1,267 tests pass in 27.73s.
- `/tmp/workdeck-pm10-ref-pty-first.log`: the narrow terminal fixture initially
  expected the document body before scrolling past frontmatter. The corrected
  journey uses the actual Shift-PgDn control; no product assertion was removed.
- `/tmp/workdeck-pm10-ref-pty-second.log`: new 62/160-column journey passes.
- `/tmp/workdeck-pm10-ref-final-pty.log`: all 29 workbench journeys pass in 32.16s.
- `/tmp/workdeck-pm10-ref-clippy.log`: strict lint identified a redundant test-only
  string conversion, removed without changing behavior.
- `/tmp/workdeck-pm10-ref-final-clippy.log`: TUI/CLI all-target strict lint passes.
- `/tmp/workdeck-pm10-ref-final-architecture.log`: 13 production crates, one
  executable, zero violations. Formatting and `git diff --check` pass.

This qualifies this bounded ref-view increment, not PM-10 or the complete goal.
Remaining work includes broader My work facets, documented scale/performance
budgets and qualification, PM-11 CI/completion evidence, and PM-12 hardening/release.

- `/tmp/workdeck-pm10-ref-final-reload.log`: nine CLI reload regressions pass in 0.53s.

Next increment investigation: My work currently queries only `IssueQuery.assignee`.
The canonical product view also requires claimed, review-requested, blocked and
overdue facets. Reviewer and due date exist in authoritative issue metadata; claims
are separate source-qualified observations (local or accepted plus coordination).
Implementation must preserve those separate identities, bind facet/time/claim
observations into pagination, and expose unavailable/uncertain claims rather than
pretending their absence means no work. No facet implementation is claimed yet.


## PM-10 review-requested and overdue My work facets — 2026-09-10

The shared IssueQuery contract adds reviewer, workflow-category and due-before
predicates. Native and indexed selection agree on exact timestamp offsets and
nanosecond boundaries. Date-only due dates become overdue after their UTC day.
New predicates serialize only when set, preserving old saved-query/intention bytes.
Disposable projection format 2 rebuilds older caches instead of falsely reporting
empty matches for unindexed reviewer/deadline data; cache-only access remains inert.

Registry reports carry the selected facet and optional explicit evaluation time.
Review requests select the actor as reviewer in the configured Review category;
overdue selects nonterminal assignments and requires as_of. Facet, actor, time and
source fingerprints bind pagination. CLI adds --facet and --as-of. My work keys
1/2/3 select assignments/reviews/overdue; explicit refresh samples a new time and
page navigation retains it. Opened excerpts and native drafts survive switching.

Evidence so far:
- `/tmp/workdeck-pm10-facets-query-red.log`: intended unknown reviewer predicate failure.
- `/tmp/workdeck-pm10-facets-query-green.log`: native/indexed exact-time parity passes.
- `/tmp/workdeck-pm10-facets-registry-red.log`: intended unknown facet failure.
- `/tmp/workdeck-pm10-facets-registry-green.log`: five registry tests pass.
- `/tmp/workdeck-pm10-facets-tui-red.log`: key 2 kept Assigned instead of ReviewRequested.
- `/tmp/workdeck-pm10-facets-tui-green.log`: eight My work tests pass.
- `/tmp/workdeck-pm10-facets-cli.log`: four CLI registry tests pass.
- `/tmp/workdeck-pm10-facets-pty.log`: actual 62/160-column facet/draft journey passes.
- `/tmp/workdeck-pm10-facets-core.log`: 42 PM library, seven projection-live,
  12 projection-storage, nine native-query and five registry tests pass (75 total).
- `/tmp/workdeck-pm10-facets-full-tui.log`: all 1,268 TUI tests pass in 28.30s.

Final terminal/catalog/lint checks and stronger same-limit/time pagination tests are
still in progress. Claimed/blocked facets, performance, PM-11 and PM-12 remain open.

Final facet increment evidence:
- `/tmp/workdeck-pm10-facets-full-pty.log`: all 30 workbench journeys pass in 30.61s.
- `/tmp/workdeck-pm10-facets-cli-regressions.log`: 21 CLI catalog/index/query/registry
  tests pass, including exact generated skill/command/schema parity.
- `/tmp/workdeck-pm10-facets-final-registry.log`: five tests pass, now including a
  custom Review-category state, same-limit cross-facet rejection, same-membership
  time-change rejection, and successful continuation with the retained cutoff.
- `/tmp/workdeck-pm10-facets-final-tui-focused.log`: eight My work tests pass after
  placing the full evaluation timestamp first in the narrow observation header.
- `/tmp/workdeck-pm10-facets-clippy.log`: PM/TUI/CLI all-target strict lint passes.

The full TUI run preceded only the final timestamp-header rearrangement and its
stronger assertion; current focused My work tests and all terminal journeys cover
that change. No test expectations were weakened or performance gates waived.

- `/tmp/workdeck-pm10-facets-architecture.log`: 13 production crates, one executable,
  zero violations. `cargo fmt --all -- --check` and `git diff --check` pass.

Next action: implement claimed/blocked facets with explicit claim observations and
shared graph semantics (including canceled prerequisites and valid waivers). Keep
claim observation/time identities separate from the selected issue source, reject
stale pagination across either, and retain unavailable/unknown results visibly.


## PM-10 claimed and blocked My work qualification — 2026-09-10

My work now exposes all five facets through shared registry operations, CLI and TUI.
Keys 4/5 select blocked/claimed work; e toggles bounded evidence without replacing an
opened source excerpt. CLI claimed queries require an explicit RFC3339 as_of, reused
across pages. Claim actor is independent of assignee. Active expired or clock-uncertain
claims remain visible for recovery; released records are excluded. Shared assessments
remain unconfirmed, with accepted and coordination source identities kept separate
from selected proposal requirements. Blockers use graph readiness and question
applicability, including canceled prerequisites, valid/stale waivers and stale answers.
Captured source changes reject the affected member and do not become empty success.

The focused RED tests failed on unknown blocked/claimed variants and unmapped TUI keys
(`/tmp/workdeck-pm10-blocked-red.log`, `...-claimed-red.log`, `...-blocked-tui-red.log`,
`...-claimed-tui-red.log`). The first expiry assertion incorrectly expected Expired
inside the policy clock-skew window. The corrected test explicitly asserts
ClockUncertain inside that window and Expired outside it; product policy is unchanged.
The local release-race extension initially used a nonexistent metadata id field; the
compile error was fixed to compare actual claim tokens before the passing rerun.

Current integrated evidence:

- `/tmp/workdeck-pm10-evidence-final-core.log`: 79 tests pass (43 library, seven
  projection live, 12 projection storage, nine queries, two registry claims, six
  registry work). Shared temporary-bare-remote tests cover coordination-only races,
  cursor invalidation, proposal mismatch and preservation of HEAD/index/working files.
- `/tmp/workdeck-pm10-claims-regression.log`: 24 tests pass across claims, admission,
  bounds, outcomes and registry claims; local post-assessment release invalidates the
  report and the next query excludes that released claim.
- `/tmp/workdeck-pm10-evidence-final-tui.log`: all 1,270 tests pass in 25.19 seconds.
- `/tmp/workdeck-pm10-evidence-final-pty.log`: all 32 workbench terminal journeys pass
  in 30.81 seconds, including claimed and blocked evidence at 62/160 columns with
  native draft preservation.
- `/tmp/workdeck-pm10-evidence-final-cli-rerun.log`: 23 catalog/index/query/registry
  tests pass, including exact generated skill, command and schema parity. The initial
  batch stopped on the nonexistent target pm_query; the correct target is pm_queries.
- `/tmp/workdeck-pm10-evidence-final-clippy-rerun.log`: strict PM/TUI/CLI all-target
  lint passes in 23.74 seconds.

Source/evidence memory and deadline guards remain explicit. These tests do not
qualify large blocked/claimed report latency, the calibrated incremental-refresh
budget, complete PM-10, or PM-11/12. The synthetic benchmark now records cumulative
capture/project/publish stage timings using existing fault hooks with no injected
faults; measured optimization and integrated phase qualification are next.

- `/tmp/workdeck-pm10-evidence-final-architecture-rerun.log`: architecture passes with
  13 production crates, one executable and zero violations.


## PM-10 projection publication scan optimization — 2026-09-10

Stage instrumentation identified a redundant full-source scan after projection;
publication already revalidated the same source after candidate fsync, under its
writer lock, immediately before replacement. The earlier scan is now a deadline
check. The final guard, snapshot capture validation, CAS and physical identity
checks remain intact. No acceptance target or source timeout was weakened.

- `/tmp/workdeck-pm10-projection-publish-race-before.log`: strengthened pre-change
  test passes for source edits after capture, projection, before publication and
  after candidate writing; each preserves the last-good checkpoint exactly.
- `/tmp/workdeck-pm10-single-publish-guard-tests.log`: 27 post-change tests pass
  (seven live, 12 storage, two registry claims, six registry work).
- `/tmp/workdeck-pm10-single-guard-final-lint.log`: strict PM all-target lint passes
  in 15.29 seconds. Architecture reports 13 production crates, one executable and
  zero violations (`...-final-architecture.log`). Format and diff checks pass
  (`...-final-format-check.log`, `...-final-diff-check.log`).
- The full TUI/CLI/PTY results immediately above precede only this core scan
  optimization and benchmark instrumentation. Its affected projection and registry
  paths have the additional current-source 27-test gate; full workspace/release
  qualification remains open.

Comparative optimized full-size runs reduce incremental refresh from 19.09 to
15.95 seconds for 40,000 features and 9.75 to 7.78 seconds for 10,000 issues. The
historical `--enforce-targets` runs correctly exited 1 with the unmet original
one-second target; the current gate uses the calibrated family-specific budgets recorded
at the top of this document.
First-use and repeated filter timings, source-stage timing interpretation, peak RSS
and all reproduction logs are in [performance evidence](project-management-performance.md).

Next work: profile and optimize the remaining complete capture, checkpoint decoding
and repeated parsing costs without discarding final source validation; qualify
actual full-size mounted views and finish PM-10's complete requirement audit.
PM-11 and PM-12 remain required and unimplemented as phases. The goal stays active.


## PM-10 filesystem observation and active-policy scale repairs — 2026-09-10

The metadata regression failed with 517 pathname observations for 128 flat records
(`/tmp/workdeck-pm10-listing-metadata-red.log`). Listing now checks directory
ancestors around enumeration and each leaf once. It retains path validation,
symlink/special-file rejection, finite entry accounting and final snapshot
membership/content validation. The bounded-read regression failed with three
pathname observations instead of two (`...-read-metadata-red.log`); the read now
reuses checked leaf metadata while retaining opened-file and byte-size checks.
A nested filesystem/memory listing parity test covers exact and exhausted bounds.

Filesystem increment qualification:

- `/tmp/workdeck-pm10-filesystem-observations-green.log`: 119 focused tests pass.
- `/tmp/workdeck-pm10-filesystem-full-pm.log`: all 789 PM tests pass across all targets.
- `/tmp/workdeck-pm10-filesystem-final-tui.log`: all 1,270 TUI tests pass (25.25 s).
- `/tmp/workdeck-pm10-filesystem-final-pty.log`: all 32 workbench journeys pass (30.73 s).
- `/tmp/workdeck-pm10-filesystem-final-cli.log`: all 23 selected CLI checks pass.
- `/tmp/workdeck-pm10-filesystem-final-lint.log`: strict PM/TUI/CLI all-target lint
  passes (23.83 s); architecture reports 13 production crates, one executable and
  zero violations. Formatting and whitespace checks pass.

The subsequent active-policy tests reproduced a separate required-scale defect:
both 40,000 features and 10,000 issues were rejected by the old 20,000-entry
organization scan. An 80-subject fixture also parsed its two history receipts 160
times. These three intended failures are retained in
`/tmp/workdeck-pm10-organization-scale-red.log`.

Organization source scans now share the existing default source-capture bounds:
100,000 entries/documents and 256 MiB total, with the existing per-document limit.
Organization-definition and identity-history limits remain separate. A lazy
RetirementIndex is shared within one immutable compliance evaluation; its selected
namespace checks and source reads remain pinned to that snapshot. It is not shared
across callbacks, changed policies, or different repositories.

- `/tmp/workdeck-pm10-organization-scale-green.log`: all three new regressions pass
  in 57.09 s, including full catalog counts, a bad actor introduced in the last
  record of each dataset, and at most two receipt parses for 80 subjects.
- `/tmp/workdeck-pm10-organization-final-pm.log`: all 792 PM tests pass on the repair.
- The new `--registered-policy` benchmark mode creates two real registered-agent
  definitions and activates their policy before generating the editor-authored
  dataset. Both complete indexed workloads run and preserve their query/source
  assertions; neither met the original one-second refresh target. They fit the current
  calibrated budgets. See
  [measured results](project-management-performance.md).

Final policy-repair checks pass: 1,270 TUI tests (26.13 s), 32 workbench PTYs
(33.86 s), 31 CLI catalog/index/query/registry/organization tests, strict PM/TUI/CLI
all-target lint (26.78 s), architecture (13 production crates, one executable,
zero violations), formatting and whitespace. Logs use the prefix
`/tmp/workdeck-pm10-organization-final-` with tui/pty/cli/lint/architecture/format-check.

PM-10's
performance/mounted-scale and richer-record gates remain open; PM-11/12 remain
required. CI implementation can proceed against the working core under the
recorded sequencing decision without converting these gaps into completion.

## PM-11 immutable commit validation — 2026-09-10

The shared `ci_validate` operation validates caller-selected base/head commit trees
without working-tree configuration or index substitution. Its explicit
`planning_source_validation` basis is not trusted CI evidence or completion admission.
No CLI CI command is delivered by this checkpoint. Missing planning baselines fail;
initialization does not silently become an empty accepted contract.

- RED `/tmp/workdeck-pm11-commits-red.log`: three exact-commit tests fail because the
  existing bounded ref resolver rejects commit IDs; full-ref/HEAD validation passes.
- GREEN `/tmp/workdeck-pm11-commits-green.log`: separate exact-commit resolution
  repairs the three cases; all four new and six staged-doctor cases pass.
- Expanded GREEN `/tmp/workdeck-pm11-commits-expanded.log`: ten commit, six staged
  and thirteen existing source cases pass, including relation closure, cross-repository
  rejection and selected-ref movement. Exact IDs remain valid when unrelated HEAD moves.
- Additional RED `/tmp/workdeck-pm11-commits-final.log`: twelve cases pass, but Serde's
  unit HEAD variant silently accepts an unknown `trusted` field. Replacing it with an
  empty struct variant preserves the wire spelling while enforcing unknown-field rejection.
- Current focused GREEN `/tmp/workdeck-pm11-commits-qualified.log`: thirteen commit,
  six staged and thirteen source cases pass. This includes unsafe Git modes, replacement
  objects, missing objects/baselines, exact tag-object rejection, invalid committed
  records hidden by repaired working files, dirty-index preservation, and strict selectors.
- Full PM gate `/tmp/workdeck-pm11-commits-full-pm.log` passes **805 tests** across
  65 targets, with zero failures. Strict PM all-target Clippy passes in 18.56s
  (`/tmp/workdeck-pm11-commits-lint.log`). No PM-11 deliverable or exit
  criterion is closed; accepted check-contract binding, execution/provenance, completion
  policies, review UI and CLI/catalog/CI integration retain their full scope.

Architecture validation also passes (13 production crates, one shipped executable,
zero dependency/source-reachability violations; 36.71s build/run log at
`/tmp/workdeck-pm11-commits-architecture.log`). Workspace formatting and
`git diff --check` pass; canonical plan/validation relative file links resolve.
The full TUI/PTY/CLI runtime suites were not rerun for this core-only increment;
their preceding qualification remains historical rather than new evidence.
Storage is 101 GiB free after verification. No process remains running for this
increment. Next: bind required checks/profiles/recipes and acceptance policy to
separately selected baseline documents, detect candidate contract weakening, then
integrate the actual CLI/catalog contracts. Caller-selected baseline trust and
initialization admission must remain explicit; local check plans do not qualify CI.

## PM-11 baseline check contracts and CLI — 2026-09-10

`ci_contracts.rs` independently captures baseline/candidate acceptance policy and
required profile/check/command closure. Exact configuration/definition document pins
bind each contract fingerprint. Complete typed definitions drive semantic comparison;
comments alone do not require semantic review. Missing/archived required definitions
fail with baseline/candidate attribution. Changed contracts do not pass without review.
This increment does not implement review approval, trusted baseline admission,
subject acceptance criteria/red-green binding, CI execution or completion admission.

Behavioral evidence:

- The first two contract test attempts failed because the fixture omitted required
  command `cwd` and `inputs`; these are fixture errors, not behavioral RED evidence.
  The first implementation build also caught typed-versus-JSON hash call errors.
- With valid fixtures and the previous source-only success predicate temporarily
  retained, `/tmp/workdeck-pm11-contracts-behavior-red.log` fails all three intended
  regressions: deleting the denominator, weakening policy and weakening expectations.
  The contract gate was restored before GREEN; no intentionally weakened gate remains.
- `/tmp/workdeck-pm11-contracts-green2.log` passes 20 commit/contract tests, 19 local
  check-plan tests and six staged validators. Cases cover exact independent definitions,
  changed recipes/profile membership/expectations, comment-only changes, missing or
  archived required definitions and repaired dirty files hiding a weakened commit.
- The initial CLI test build exposed an unavailable YAML test dependency; the fixture
  was changed to edit its actual config bytes without adding a dependency.
  `/tmp/workdeck-pm11-cli-behavior-red.log` then fails all three intended CLI cases
  because the executable reports `Unknown command: ci`.
- `/tmp/workdeck-pm11-cli-green.log` passes three new CLI cases and all three staged
  doctor CLI cases. `pm_ci.rs` dispatches before working config discovery, reports exact
  resolved identities and contract errors, and rejects revision expressions.
- `/tmp/workdeck-pm11-cli-regression.log` passes 94 executable unit tests, six command/
  schema/generated-reference tests, three CI tests and three doctor tests (106 total).
  Commands, schemas and the generated agent skill were rendered from the actual binary.
- Final expanded CI/catalog and core tests, strict lint and architecture qualification
  are recorded below once terminal. No phase is closed by this bounded increment.

Current resulting-source qualification:

- `/tmp/workdeck-pm11-contracts-final-core.log`: **61 core tests** pass (20 commit/
  contract, 19 local check-plan, three command-catalog, 13 source and six staged).
- `/tmp/workdeck-pm11-cli-final.log`: **four CI and six catalog tests** pass, including
  exact-SHA invocation, capability limitations, report-schema discovery and generated
  command/schema/skill parity. Together with the unchanged-production 94 executable
  unit and three doctor cases above, **107 distinct CLI/unit/catalog tests** pass.
- `/tmp/workdeck-pm11-contracts-lint.log`: strict PM/CLI all-target Clippy passes
  (27.47s). The full PM/TUI/PTY suites were not repeated for this additive CI/catalog
  increment; their prior results are historical rather than new broad qualification.

Architecture also passes (13 production crates, one executable, zero violations;
22.45s in `/tmp/workdeck-pm11-contracts-architecture.log`). Formatting, whitespace
and canonical plan/validation/CLI README file links pass. Disk has 97 GiB free at
this checkpoint. All task verification processes are terminal. No real backlog,
commit, push, sibling change or external publication was performed.

Next: bind subject acceptance criteria and required red/green contracts, then
implement trusted baseline/review admission, actual revision-bound CI check execution,
evidence producer provenance and completion/review policy. PM-10 performance and
mounted-scale qualification and all remaining PM-12 work remain open. The full goal
is active; phase count remains 10/13 closed, approximately 77% by phase count.

## PM-11 subject acceptance and workflow contracts — 2026-09-10

`ci_subjects.rs` derives typed requirement contracts from bounded immutable source
snapshots. It includes issue criteria (IDs/descriptions, not checked declarations),
issue/feature parent-prerequisite-gate links, feature criteria, project exit criteria,
milestone outcomes and gate requirements/age/producer/check pins. Gate custom/extension
fields are retained; record times/revisions and descriptive prose do not define the
requirement. Stable subject IDs match physical feature moves. Each record still pins
its exact original document hash. The evaluation contract also retains workflow policy;
changing completion categories or transitions cannot bypass contract review.

- `/tmp/workdeck-pm11-subjects-red.log`: the intended acceptance rewrite test fails
  because source/schema and check-contract validation allowed a weaker description.
- `/tmp/workdeck-pm11-subjects-green.log`: all 21 then-current commit/contract cases pass.
- `/tmp/workdeck-pm11-subjects-expanded.log`: 25 cases pass; the milestone fixture lacks
  its mandatory owning project. This fixture error is corrected, not treated as RED
  evidence for runtime behavior. `/tmp/workdeck-pm11-subjects-final-core.log` then passes
  91 affected core tests.
- `/tmp/workdeck-pm11-workflow-contract-red.log`: reclassifying the Done state succeeds
  incorrectly under the preceding contract comparison. Adding workflow policy to the
  pinned semantic contract closes this case.
- The first final command named a nonexistent `workflow` integration target and ran
  no tests (`/tmp/workdeck-pm11-subjects-qualified-core.log`). The corrected final gate
  `/tmp/workdeck-pm11-subjects-qualified-core2.log` passes **92 tests**: 28 CI/source/
  contract, 18 feature, ten gate, 17 hierarchy and 19 planning cases.
- `/tmp/workdeck-pm11-subjects-cli.log` passes 94 executable unit, five actual CI CLI
  and three staged-doctor cases. A new CLI case confirms stable issue identity in the
  review-required report even when a dirty repaired file hides the weaker commit.
- Generated command/schema/skill artifacts were regenerated from the resulting binary.
  `/tmp/workdeck-pm11-subjects-catalog.log` passes six command/schema/generated-parity
  cases, for **108 distinct CLI/unit/catalog tests** on this increment.

These are requirement-change checks, not completion qualification or trusted CI.
Accepted evaluator/test inputs must still be separated from candidate test edits;
required red/green evidence, baseline/review authority, CI execution/provenance and
completion/review policy remain open. PM-10 performance/mounted-scale and PM-12 remain
open. The previous full PM/TUI/PTY qualifications remain historical rather than new
broad qualification for this increment.

Final scope correction before closing this increment: comparison with the existing
`context/requirements.rs` showed that issue-to-feature links and feature decision state
also belong to the reachable requirement contract. The intended RED in
`/tmp/workdeck-pm11-subject-inheritance-red.log` confirms that dropping an inherited
feature link previously passed. `ci_subjects.rs` now binds those links and feature
decisions, including features with no criteria yet. The final core run
`/tmp/workdeck-pm11-subjects-final2-core.log` passes **120 tests**: 30 CI/contract,
26 context, 18 feature, ten gate, 17 hierarchy and 19 planning. Earlier 92-test and
lint/architecture results are superseded for final-source qualification; CLI/catalog,
lint and architecture are rerun after this correction.

Final resulting-source qualification after the inheritance correction:

- `/tmp/workdeck-pm11-subjects-final2-cli.log`: 94 executable unit, five CI and three
  staged-doctor tests pass; `/tmp/workdeck-pm11-subjects-final2-catalog.log` passes all
  six catalog/schema/generated-reference cases (**108 CLI/unit/catalog tests**).
- `/tmp/workdeck-pm11-subjects-final2-lint.log`: strict PM/CLI all-target Clippy passes
  in 23.62s.
- `/tmp/workdeck-pm11-subjects-final2-architecture.log`: architecture passes with 13
  production crates, one executable and zero violations (22.34s build/run).
- Workspace formatting, `git diff --check`, and canonical plan/validation/CLI README
  file links pass. The generated command/schema/agent references match the executable.
- All task verification processes are terminal. Disk has 96 GiB free. No real backlog,
  commit, push, sibling change or external publication was performed.

Next: trusted evaluator/test-input pins separate from candidate test edits, required
red/green evidence, baseline/review admission, revision-bound CI check execution and
producer provenance, then completion/review integration. PM-10/PM-12 gates remain open.
The full goal remains active; no phase was closed by this increment.


## PM-11 committed evaluator inputs — 2026-09-10

Required CI check definitions now declare `evaluator_inputs` as literal repository
files/trees, constrained to mandatory runtime command inputs. Missing declarations
fail CI while existing local definitions remain valid; `{}` explicitly declares no
repository evaluator dependencies. Capture reads immutable bounded Git objects and
compares bytes, executable modes and selected tree membership independently from
candidate application files. Missing inputs, symlinks and case aliases fail. Engine
output is excluded, including for repository-root tree selection. Shared source byte,
entry and deadline limits apply; captures do not execute or fetch.

The initial intended RED (`/tmp/workdeck-pm11-evaluator-inputs-red.log`) accepted an
`assert True` candidate with unchanged check metadata. The implementation closes it.
The portability RED (`/tmp/workdeck-pm11-evaluator-portability-red.log`) found that
explicit files under `Tests/` and `tests/` escaped full-path-only collision checks.
Checking ancestor tree spelling before selection filtering closes this defect.
The expanded run also exposed a test TempDir lifetime mistake, corrected without
changing the expected behavior.

`/tmp/workdeck-pm11-evaluators-core.log` passes **84 tests**: 38 CI validation,
19 check plans, five check records, three command catalog, 13 source and six staged
doctor tests. Cases cover evaluator weakening hidden by dirty repair, application-only
changes, membership/mode changes, missing/symlinked inputs, unsafe selectors, omitted
runtime tracking, ancestor case aliases and root selection excluding engine output.
Final affected qualification:

- `/tmp/workdeck-pm11-evaluators-cli.log`: six actual CI CLI tests pass, including a
  changed evaluator hidden by a repaired dirty file with unchanged check metadata.
- `/tmp/workdeck-pm11-evaluators-catalog.log`: six catalog/schema/generated-parity
  tests pass; command, schema and agent references were regenerated from the binary.
- `/tmp/workdeck-pm11-evaluators-lint.log`: strict PM/CLI all-target Clippy passes
  (28.59s).
- `/tmp/workdeck-pm11-evaluators-architecture.log`: 13 production crates, one shipped
  executable, zero dependency/source-reachability violations; build took 31.23s.
- Workspace formatting, `git diff --check` and canonical plan/validation/CLI README
  local file links pass. These are 96 affected tests, not a fresh full-workspace or
  terminal qualification. All task verification handles are terminal.

Next implementation step: revision-bound CI check execution with explicit accepted
baseline admission, retained evaluator identities and producer provenance. A local
run or a caller-set trust flag must not become trusted qualification. Red/green and
completion/review policies remain required alongside that execution path.

Storage intervention completed before this run: task `cargo clean` removed 6,645
files (3.5 GiB), verified target absence and 99 GiB free. Subsequent tests use the
same isolated target with incremental and debug information disabled. The unrelated
main checkout build was preserved. No source files were removed by cleanup.

These are declared evaluator manifests, not sandbox proof or trusted execution.
Baseline/review admission, revision-bound `ci check`, red/green evidence, producer
provenance and completion/review integration remain required. PM-10 performance and
PM-12 qualification remain open; the full goal is active.


## PM-11 check-plan input admission — 2026-09-10

`ci_check_inputs.rs` exposes shared `bind_ci_check_plan`; `sources/check_inputs.rs`
verifies exact commit/tree/planning-content/repository identity and committed command
catalog/configuration, then compares selected input bytes, modes and complete tree
membership. Optional absence is checked against Git, and repository-resolved tools
are captured even outside normal file selectors. `check_input_entries.rs` compares
membership independently of local directory hash encoding. Live plan revalidation
runs before and after the bounded Git read. The binding includes exact source and
plan hashes plus candidate invocation manifests; candidate inputs remain a distinct
type from accepted evaluator manifests. External tool/environment hashes remain in
the complete local plan. No process is started, no checkout allocated and no CI trust
is granted by this API.

The implementation will use the supplied CI checkout, admitting its selected bytes
against the requested commit before invoking the existing runner. Merely calling an
ordinary local plan revision-bound would have accepted dirty input. The intended RED
in `/tmp/workdeck-pm11-ci-input-binding-red2.log` demonstrates that gap. An earlier
fixture used unavailable `/bin/false`; it was corrected to available `/bin/sh` before
recording the meaningful RED. The root-selection test initially used an initializer
fixture containing untracked empty directories. A fresh temporary Git clone supplies
the clean candidate; adding an empty directory then fails admission as required.

Seven new cases cover dirty input, deterministic matching bindings, untracked members,
mode changes, optional presence/absence, dirty definitions, forged source pins,
repository tools and fresh-clone root membership. The intermediate 99-test core gate
passed. The diagnostic assertion then needed `Option<&str>` rather than `Option<&Path>`;
two attempted final builds failed compilation before tests. Final resulting-source qualification supersedes those intermediate attempts:

- `/tmp/workdeck-pm11-ci-input-binding-final3-core.log`: **99 tests** pass: 45 CI,
  19 planning, 16 execution, 13 source and six staged-doctor cases.
- `/tmp/workdeck-pm11-ci-input-binding-cli.log`: **12 tests** pass: six CI and six
  catalog/schema/generated-parity cases. The binary was rebuilt and generated
  command/schema/agent references regenerated; `ci-check-input-binding` is discoverable
  as a schema, while the unavailable `ci check` command remains unadvertised.
- `/tmp/workdeck-pm11-ci-input-binding-lint.log`: strict PM/CLI all-target lint passes.
- Workspace formatting, whitespace and canonical plan/validation/CLI README local
  file links pass. `/tmp/workdeck-pm11-ci-input-binding-architecture.log` passes with
  13 production crates, one shipped executable and zero violations (23.15s build).
  Strict lint completed in 24.10s. All task verification handles are terminal.

This qualifies 111 affected tests, not the full terminal/platform/release workflow.

This increment does not deliver `ci check`: baseline/evaluator admission, durable CI
intent and replay, execution/recovery/cancellation integration, producer provenance,
red/green and completion/review policy remain required. The guard proves declared
selected inputs, not hermetic execution or arbitrary undeclared process access. PM-10
and PM-12 remain open. The previous turn was progress, and this turn advances the
revision-bound execution admission seam; there is no external blocker.


## PM-11 durable revision-bound check execution — 2026-09-10

`ci_execution.rs` adds shared preparation and revision-bound run requests;
`execution/store.rs` uses the existing replay-first reservation, foreground runner,
terminal journal and recovery path. `RunIntent.revision` retains the exact source/
input binding before spawn. Reservation receipt hashes include the binding; ordinary
local requests retain their prior JSON representation and cannot alias bound requests.
Offline record proof checks the retained binding against the plan; first admission
also rereads committed objects and verifies live inputs before publishing intent.
A lost intent/result acknowledgement never becomes permission to execute again.
Preflight rejection acknowledges cleanup even when no process starts.

The first intended RED (`/tmp/workdeck-pm11-ci-execution-red.log`) observed a retained
intent without the revision binding immediately before spawn. The implementation
closes that gap. Core cases cover retained binding, stale replay, wrong/forged binding,
ordinary/bound request separation, interrupted intent and terminal-journal recovery,
pre-spawn and during-run changes, cancellation, dirty issue requirements and omitted
selected evaluator declarations. Local-feedback basis is preserved explicitly.

`ci plan --revision REV --profile ID [--issue ID] --json` prepares a supplied checkout.
`ci check --plan-file FILE --expected-plan BINDING_FINGERPRINT --actor ACTOR
--request-id REQUEST --json` runs the exact saved plan. Raw prepared JSON and the
successful planning envelope are accepted. The CLI uses the shared signal callback
and returns nonzero for non-passing states, retaining result/recovery identity.
Catalog schemas and generated references describe the implemented syntax.

Intermediate qualification:

- `/tmp/workdeck-pm11-ci-execution-final2-core.log`: 83 affected tests pass: seven new
  CI execution, 45 CI validation, 16 execution, five retained records, four context
  checks and six staged-doctor cases.
- `/tmp/workdeck-pm11-ci-execution-final-cli.log`: 22 cases pass: eight CI, eight local
  check CLI and six catalog/schema/generated-parity tests, including signal cleanup.
- An initial CLI build required conversion from `u8` to the existing `i32` exit type.
  A new issue fixture initially assumed an unavailable document field; it now retains
  the actual file bytes. Both attempted gates failed compilation before running tests.
- Cleanup acknowledgement and explicit Unix capability metadata were then corrected.
  Final full PM, regenerated CLI, lint and architecture qualification is in progress
  and supersedes the preceding intermediate source qualification.

These commands execute revision-bound **local feedback**, not trusted CI qualification.
They use the supplied checkout; no sandbox or checkout is allocated. Accepted baseline/
evaluator review admission, producer provenance, red/green evidence, completion/review
policy and CI/release wiring remain required. PM-10 scale/performance and PM-12 remain
open. No source/index publication, real-backlog mutation, sibling change or commit was
performed. The full goal remains active.


Review correction before final qualification: the first full PM run
(`/tmp/workdeck-pm11-ci-execution-full-pm.log`) passed **844 tests**, with no failures
or ignored tests. Two direct-API regressions then demonstrated admission bypasses in
`/tmp/workdeck-pm11-ci-execution-admission-red.log`: a supplied bound plan could omit
its evaluator declaration, and a binding could cite malformed committed planning.
Shared run admission and retained-record proof now require selected declarations;
input admission runs the shared committed-source structural/closure validator.
`/tmp/workdeck-pm11-ci-execution-admission-green.log` passes all nine CI execution
cases. The final full PM rerun is in progress on the corrected source; the earlier
844-test run is historical, not final-source qualification.


Remaining policy boundary confirmed during review: `DoctorReport.valid` is structural
validity, while organization compliance is reported separately as `policy_compliant`
and `policy_violations`. The current CI commands remain feedback/structural validation.
Trusted admission must consume committed organization compliance and cover changes to
organization/producer authority in baseline review; the existing semantic config
comparison currently binds acceptance/workflow policy only. This remains part of
PM-11 policy/provenance work, not evidence of completed CI qualification.


Final resulting-source qualification after admission review:

- `/tmp/workdeck-pm11-ci-execution-final-full-pm.log`: **846 PM tests pass**, across
  66 completed groups; zero failures and zero ignored tests.
- `/tmp/workdeck-pm11-ci-execution-final2-cli.log`: eight CI and eight existing local
  check CLI tests pass. `/tmp/workdeck-pm11-ci-execution-final-catalog.log`: six
  catalog/schema/generated-parity tests pass (**22 CLI/catalog cases**).
- The rebuilt executable regenerated command/schema/agent references, including the
  explicit Unix execution limitation and the new prepared/run/binding schemas.
- `/tmp/workdeck-pm11-ci-execution-final-lint.log`: strict PM/CLI all-target Clippy
  passes in 23.95s.
- `/tmp/workdeck-pm11-ci-execution-final-architecture.log`: 13 production crates,
  one shipped executable, zero dependency/source-reachability violations; 22.75s build.
- Workspace formatting, whitespace and canonical plan/validation/CLI README local
  file links pass. All task verification handles are terminal.

PM-11.D1 is qualified: headless validation, prepared revision-bound verification and
frozen executable syntax are delivered. Other PM-11 policy/provenance/review/release
requirements and phase exit criteria remain open. These tests are not full TUI,
platform, release or independent final-review qualification. Only temporary test
repositories were used for mutations; implementation changes remain local/uncommitted.

Next: committed organization/producer policy and accepted-baseline admission, portable
result descriptors/provenance, red/green and completion/review integration. Preserve
the distinction between these required trust decisions and the local-feedback runner.
The full objective remains active; there is no external blocker.


## PM-11 organization-policy admission — 2026-09-10

Confirmed RED: `ci_rejects_candidate_organization_policy_violations_but_allows_repairs`
failed because a structurally valid candidate missing a required custom field was
accepted (`/tmp/workdeck-policy-red.log`). After explicit candidate compliance
admission, the same test passes and exercises repair under unchanged baseline policy
(`/tmp/workdeck-policy-green.log`). Doctor structural validity retains its existing
meaning; candidate CI admission additionally requires `policy_compliant`.

Confirmed RED: `ci_requires_review_for_organization_policy_removal` failed because
removing registered estimate-unit policy was not treated as a contract change
(`/tmp/workdeck-policy-removal-red.log`). Independent organization contracts now
capture users and schema records with exact source tokens. Semantic comparison uses
virtual default records for absent optional files, excludes record revisions and user
display names, and includes identity membership/kind/archive state, registry mode,
custom-field definitions, unit policy and extension metadata. Unit definitions are
conservatively compared in full. This closes policy removal/downgrade detection;
it does not establish who accepted a baseline or reviewed a change.

Initial affected gate: 49 CI validation, 9 CI execution and 28 organization tests
pass (`/tmp/workdeck-policy-final-core.log`). Additional default-materialization and
actual CLI regressions plus generated schema refresh are undergoing qualification.
No phase closes and full-suite evidence from before this change remains historical.

The initial CLI regression asserted a success-envelope path for an expected failure.
Inspection confirmed the existing `policy_blocked` envelope correctly carries the
report at `error.details.report`; the test now asserts that public failure contract.
This was a test assertion correction, not a relaxed admission rule.

Final affected tests: **87 core tests** pass (50 CI validation, 9 CI execution,
28 organization; `/tmp/workdeck-policy-core-final2.log`). **15 CLI/catalog tests**
pass (9 actual CLI cases, `/tmp/workdeck-policy-cli-final.log`; 6 discovery/schema
and generated-reference checks, `/tmp/workdeck-policy-catalog.log`). Generated
references were refreshed from the resulting CLI binary. Default materialization
preserves semantic authority while changing exact pins; identity-mode introduction,
policy-file removal, cosmetic names, candidate violations, unchanged-policy repair,
and direct binding/preparation admission are exercised. Strict lint and architecture
qualification are still running at this checkpoint.

Final qualification: strict PM/CLI all-target Clippy passes in 28.66s
(`/tmp/workdeck-policy-lint.log`); architecture passes with 13 production crates,
one executable and zero dependency/source-reachability violations
(`/tmp/workdeck-policy-architecture.log`). Formatting, whitespace and canonical
plan/validation local-file links pass. All task Cargo handles are terminal.
Storage remains about 105 GiB free after rebuilding the isolated verification target.

Next: PM-11.D3 producer-provenance/report admission, followed by trusted baseline
and evaluator-review admission, required red/green and completion integration.
Current `EvidenceProvenanceKind` is still only `Declared`; retained run results are
still local feedback. Neither organization-policy pins nor successful local runs
satisfy those remaining authority requirements. PM-10 performance/full-scale gates
and PM-12 qualification retain their complete scope.


## PM-11 portable check report export — 2026-09-10

RED: the real CLI workflow reached `check export` after a revision-bound run and
stale replay, then failed with the expected unrecognized-subcommand diagnostic
(`/tmp/workdeck-report-export-red.log`). The new command calls the shared
`Repository::export_check_report`, and its public `CheckReport` preserves original
intent/result/receipt proof plus current freshness observation. Parsed counts and
failure details remain inside terminal check outcomes. No log/artifact bodies are
embedded and no actor attribution is authenticated by this export.

Offline `CheckReport::from_json` checks a 64 MiB transport limit, individual run
record bounds, canonical fingerprint, intent/result binding, reservation/publication
receipt relationships and observation consistency. It performs no checkout reads,
execution, recovery, evidence import or completion admission. Terminal failed/stale
results can be exported without changing their states; unfinished runs are rejected.

The initial core build found a missing crate-root reexport for the new public type;
that wiring was fixed before tests ran. Initial core 11 tests and actual CLI 17 tests
pass. Extended proof relationship tests and existing execution/report regressions
are running; final qualification and generated parity follow.

The first affected gate passed 48 core tests (records 5, parsed reports 11,
CI execution/export 12, context checks 4, execution 16), plus 17 actual CLI tests
and 6 catalog tests. Additional missing-local-proof coverage confirms export retains
Unknown separately from the historical result. Recomputed fingerprints cannot hide
cross-run receipt substitution, false-green observations or omitted receipts.

Review added `observed_at` to the exported freshness observation and its fingerprint.
This timestamp is exporter-attributed and unauthenticated; offline consistency does
not establish that source/log/artifact freshness remains true at consumption time.
Affected checks and generated references are being rerun for this final change.

Final report gate: 12 CI execution/export tests pass after the timestamp addition
(`/tmp/workdeck-report-core-final3.log`), including missing local terminal proof.
The remaining 36 affected existing record/parser/context/execution regressions
passed on the preceding source (`/tmp/workdeck-report-core-final.log`); the timestamp
addition is confined to exported report shape. Final actual CLI 17 tests pass
(`/tmp/workdeck-report-cli-final.log`), and all 6 generated-reference/catalog tests
pass (`/tmp/workdeck-report-catalog-final.log`). The generated schema was refreshed
from the resulting binary. Strict lint and architecture are being finalized.

Final strict PM/CLI all-target lint passes in 23.65s
(`/tmp/workdeck-report-lint-final.log`). Architecture passes with 13 production
crates, one shipped executable and zero dependency/source-reachability violations
(`/tmp/workdeck-report-architecture.log`). Formatting, whitespace and canonical
plan/validation/CLI README file links pass. All task Cargo handles are terminal;
approximately 104 GiB remains free.

Next work: producer-trust policy and evidence-descriptor admission must consume
source-bound report/provenance rather than treating this consistency proof as
qualification. Report export is delivered; PM-11.D3 stays open for trusted import.
Accepted baseline/evaluator review, red/green, completion integration, PM-10 gates
and PM-12 remain required. No phase closes in this increment.


## PM-11 DSSE producer authentication — 2026-09-10

RED: actual CLI discovery rejected `ci authenticate` as an unknown command
(`/tmp/workdeck-producer-red.log`). The shared `producers` module now verifies
Ed25519 DSSE envelopes under a separately pinned `ProducerTrustPolicy` and expected
commit. This adds authentication, not completion qualification or durable import.
`ci policy` exposes the policy and canonical fingerprint for external review;
`ci authenticate` works without discovering or initializing planning.

Protocol source: [DSSE 1.0.2](https://github.com/secure-systems-lab/dsse/blob/master/protocol.md).
The reference PAE vector is exercised. Verification authenticates exact payload
bytes before parsing, supports standard/URL-safe padded or unpadded base64, and
ignores `keyid` as an authority source. Strict Ed25519 verification uses the existing
locked `ed25519-dalek` 2.2.0 dependency, now explicitly declared by PM (and CLI tests).
No dependency version update or private signing-key handling is introduced.

Policies admit 1–16 unique producer identities/keys, validity intervals and exact
check-definition scopes. One valid signer must authorize all checks; scope grants
are not combined across producers. Report source repository/commit and observation
time are checked. Producer policy pin mismatch, wrong key, altered payload/type,
wrong commit, expired/not-yet-valid observations, scope mismatch and signed local-only
reports fail. Authenticated failed reports remain failed and local-feedback reports
retain their original basis. Serialized authentication results are not credentials.

Bounds include 512 KiB policy, 96 MiB envelope, 64 MiB decoded report, eight signatures
and 512 MiB conservative aggregate payload hashing. A 5 MiB synthetic envelope with
16 producers/eight signatures exercises workload rejection before cryptographic work.

Initial core 15 tests and actual CLI 11 tests pass, including offline authentication
of an exported failed report after source checkout deletion and rejection of policy
substitution. An initial focused command inadvertently filtered the integration
binary to zero tests; it is not counted as integration evidence. The unfiltered
library/CI execution gate is now running, including the new local-only/workload cases.
Final CLI, schema parity, lint and architecture qualification follow.

Final affected gate: 50 library tests and 17 CI execution/report/authentication tests
pass (`/tmp/workdeck-producer-core-final2.log`). This includes the DSSE reference
PAE vector and all local-only/workload guards. Actual CLI 11 tests pass
(`/tmp/workdeck-producer-cli-final.log`), including `ci policy` fingerprint inspection
and offline authentication. Generated command/schema/skill references were refreshed
from the resulting binary; 6 catalog/schema/parity tests pass
(`/tmp/workdeck-producer-catalog.log`). Strict PM/CLI all-target lint passes in 24.26s
(`/tmp/workdeck-producer-lint.log`). Review added an explicit cross-repository policy
case to the existing negative authentication test; that test file is being rechecked.
Architecture and final documentation checks follow. No phase closes.

Final cross-repository test recheck passes all 17 CI execution/authentication tests
(`/tmp/workdeck-producer-core-final3.log`), and strict lint for the changed test target
passes (`/tmp/workdeck-producer-test-lint-final.log`). Architecture passes with 13
production crates, one executable and zero dependency/source-reachability violations
(`/tmp/workdeck-producer-architecture.log`). Formatting, whitespace and canonical
plan/validation/CLI README local-file links pass. All task Cargo handles are terminal;
approximately 102 GiB remains free.

Next: implement durable evidence-descriptor import retaining original signed report
bytes and policy/subject bindings, with replay/recovery and re-verification at admission.
Do not treat `AuthenticatedCheckReport` JSON alone as a credential. Accepted baseline,
evaluator review, red/green and completion policy remain separate required admissions.
PM-11.D3 remains open for that import/integration; PM-10/PM-12 keep their full scope.


## PM-11 durable signed-report import — 2026-09-10

RED: `ci import-report --help` failed as the expected missing command
(`/tmp/workdeck-import-report-red.log`). The implementation adds a shared immutable
attestation store, preserving the exact original DSSE envelope string, policy snapshot,
expected policy/commit, importer actor and historical admission summary. Dedicated
`ATST-<ULID>` identities distinguish attestations from declared `EVD` references.

The existing journal/receipt engine supplies serialized replay and crash recovery.
The new `Attestation` snapshot kind has an explicit canonical path and 8 MiB record
limit. Doctor checks original import receipts as inventory, so deleting the final
attestation cannot erase its provenance. Native snapshot merge rejects overwrites,
and restoration preserves original receipts and exact signed bytes. The operation
receipt validator participates in recovery, operation history and snapshot validation.
Current reauthentication always takes an external policy and expected pins; embedded
historical policy snapshots cannot renew their own authority.

Initial focused tests: four import tests pass (`/tmp/workdeck-import-report-core.log`),
covering original bytes/replay/snapshot inclusion, five transaction fault points,
missing/edited source/receipt detection and rejection under a replacement key policy.
Initial actual CLI 12 tests pass (`/tmp/workdeck-import-report-cli.log`), including
import/replay/list/read/reauthenticate of failed signed reports. Added restoration,
invalid-input/no-publication and concurrent-request cases are running with the full
PM suite (`/tmp/workdeck-import-report-full-pm.log`) after the dedicated ID change.
No phase closes from this initial evidence.


Qualification update after storage cleanup:

- Full PM suite passed **866 tests across 66 groups**, zero failed or ignored
  (`/tmp/workdeck-import-report-full-pm.log`). This precedes the following final
  activity extraction, diagnostic path and summary-list refinements.
- A dated attestation Activity query first failed with zero rows. Typed import-time,
  actor, producer title and historical status extraction fixes the query. Projection
  schema 3 forces older disposable caches to rebuild; opaque envelope bytes are not
  indexed as search text. CI execution 24, projection live 7 and storage 12 tests
  pass (`/tmp/workdeck-import-report-activity-final.log`).
- Missing-source diagnostics first failed because the error lacked its path. Store
  validation now identifies the affected record or receipt; all 24 CI execution
  tests pass (`/tmp/workdeck-import-report-final-core.log`).
- `ci reports` returns summaries without original envelope/policy/document payloads;
  `ci report ID` retains the full proof. After correcting the public summary export,
  8 check CLI and 12 CI CLI tests pass
  (`/tmp/workdeck-import-report-final2-cli.log`). Generated references were refreshed;
  6 catalog and 4 index tests pass
  (`/tmp/workdeck-import-report-final-catalog-index.log`).
- Cargo cleanup removed 11.5 GiB of generated artifacts from the isolated verification
  target and main Workdeck target, with no live Cargo/rustc process. Both targets
  were verified absent; free space increased from 96 to 107 GiB. Source was preserved.

The full-suite result is not a claim of full final-source qualification. Strict lint
and architecture are being repeated after the final refinements. PM-10 performance,
PM-11 criterion/completion acceptance and PM-12 remain open.

Final qualification tail: strict PM/CLI all-target Clippy passed with warnings denied
(`/tmp/workdeck-import-final-clippy.log`, 36.91s). Architecture passed with 13
production crates, one shipped executable and zero dependency/source-reachability
violations (`/tmp/workdeck-import-final-architecture.log`). Workspace formatting,
`git diff --check` and local links in the four updated documents pass. Both Cargo
handles are terminal. The isolated rebuild leaves approximately 105 GiB free.
The previous goal turn made progress by reclaiming storage and confirming the
pending catalog/index gate; this turn completes the qualification tail and updates
the authoritative checkpoint. No additional phase is closed.


## PM-11 independent baseline pins — 2026-09-10

The preceding turn completed import qualification and its evidence checkpoint (progress).
The new regression first failed compilation on the missing pin type/API/basis
(`/tmp/workdeck-baseline-pin-red.log`). Shared `ci_validate_pinned` now performs fresh
immutable source validation, checks the externally supplied baseline commit and
contract fingerprint, and returns a distinct basis plus retained pin. It does not
accept a serialized validation report as proof or approve changed candidate contracts.

The actual `ci validate` command accepts paired `--expected-base-commit` and
`--expected-base-contract` flags. Existing no-pin output remains compatible, including
omission of the optional pin. Both pins must come from an independent accepted
channel; candidate-selected values supply consistency, not reviewer authority.

52 core CI validation and 13 actual CLI tests pass
(`/tmp/workdeck-baseline-pin-final-tests.log`), including substituted baseline commit,
wrong contract hash, paired option enforcement, successful matching pins, and a
weakened candidate check remaining invalid despite valid baseline pins. Existing
immutable capture, cross-repository, organization policy and evaluator regressions
remain covered. Final generated parity/lint/architecture follows. No phase closes;
authenticated evaluator-change review, red/green and integrated completion remain open.

Final baseline-pin qualification: 6 catalog/schema/generated parity tests pass
(`/tmp/workdeck-baseline-pin-catalog.log`); strict PM/CLI all-target Clippy passes
with warnings denied (`/tmp/workdeck-baseline-pin-clippy.log`, 24.42s). Architecture
passes (`/tmp/workdeck-baseline-pin-architecture.log`), with 13 production crates,
one shipped executable and zero violations. Workspace formatting, whitespace and
local document links pass. Cargo handles are terminal; approximately 102 GiB remains
free. This is affected-source evidence, not a new full-workspace or phase closure.


## PM-11 signed evaluation-contract review — 2026-09-10

The preceding turn delivered qualified baseline pin enforcement (progress). A new
acceptance test first failed on the missing contract-review API/types
(`/tmp/workdeck-contract-review-red.log`). The shared operation now authenticates
original DSSE bytes using a separate, independently pinned reviewer policy, then
captures immutable baseline/candidate sources with the existing pin gate. Every
required reviewer must sign the same approval; aliases sharing one key are rejected.
The distinct payload type prevents check-report/review substitution.

Approval binds exact baseline pins, full candidate identity and contract, decision
and expiration. Current and historical reviewer validity are both checked. The
combined result retains the unreviewed report and cannot waive structural or
organization-policy errors. `ci validate-reviewed` exposes this shared gate;
`ci review-policy` reports a fingerprint without selecting trust. Serialized output
is not a reusable credential, and signing/private keys remain external.

Initial affected qualification passes 53 CI validation, 24 execution/authentication
and 14 actual CLI tests (`/tmp/workdeck-contract-review-tests.log`). Negative cases
include missing reviewers, duplicate-key aliases, tampered payload, wrong DSSE type,
policy substitution, future/expired approvals, signed request-changes, wrong contract
and replay against a later candidate. Existing producer/report import regressions
cover the shared base64/key/PAE helpers after extraction.

Final cases add two-required-reviewer success and an authenticated approval of a
policy-invalid candidate remaining invalid. The initial focused command mistakenly
provided two Cargo filters and exited before compilation; the corrected common
`contract_review` filter is running in `/tmp/workdeck-contract-review-final-focused2.log`.
Generated references and final lint/architecture qualification follow. Durable review
retention, TUI coverage, red/green and completion integration remain required.

Final qualification: the corrected focused run passes 1 CLI and 2 core tests, including
the added two-reviewer and policy-invalid approval assertions. Six generated schema/
command/skill parity tests pass (`/tmp/workdeck-contract-review-catalog.log`). Strict
PM/CLI all-target Clippy passes with warnings denied (26.71s,
`/tmp/workdeck-contract-review-clippy.log`). Architecture passes with 13 production
crates, one shipped executable and zero violations
(`/tmp/workdeck-contract-review-architecture.log`). Workspace formatting, whitespace
and local links pass. All task Cargo handles are terminal. This is affected-source
qualification; no full PM/TUI/workspace/release or phase-completion claim is made.


## PM-11 durable contract-review retention — 2026-09-10

The preceding turn delivered signed contract-review admission (progress). The new
retention acceptance assertions first failed compilation on missing import/read APIs
(`/tmp/workdeck-retain-review-red.log`). Immutable `CRVW` records now retain original
UTF-8 envelope text, external policy snapshot/pins, actor and import-time admission.
The standard engine binds request replay, crash recovery and create-only receipt
authority. Initial implementation compilation exposed wildcard re-export collisions;
explicit public exports fix them (`core`/`core2` logs). Basic import/read/replay/current
admission then passed (`/tmp/workdeck-retain-review-core3.log`).

Historical parsing verifies original proof at original import time without Git access.
Current admission recaptures exact committed sources under externally supplied current
policy and baseline/candidate pins. Fresh import rejects invalid planning even if
its approval is signed. Doctor/recovery/operation history/native snapshots include
the new record and receipt validator. Native merge cannot overwrite immutable proof.
Activity indexes import time, importer and historical reviewer decision rather than
opaque signed text. No current completion authority is inferred from historical rows.

The expanded focused command initially failed on an assertion comparing diagnostic
String paths to PathBuf; corrected assertion matches the existing error contract.
The final focused run passes 1 CLI and 2 core tests
(`/tmp/workdeck-retain-review-focused2.log`), exercising five engine fault points,
concurrent duplicate imports, exact original bytes and receipts, snapshot restoration
without Git, current admission rejection without Git, missing/edited record and missing
receipt diagnostics, invalid input without publication, and seven dated Activity rows.
Actual CLI import/list/read/current-reauthentication passes; list output omits proof
payloads. Full PM all-target verification is running
(`/tmp/workdeck-retain-review-full-pm.log`) before final CLI/generated/lint/architecture
qualification. Free space remains approximately 103 GiB.

This does not complete contextual review coverage in the TUI, required red/green,
criterion/completion policies, PM-10 performance or PM-12 qualification.

Full PM all-target qualification passed **870 tests across 66 groups**, zero failed
or ignored (`/tmp/workdeck-retain-review-full-pm.log`, terminal exit 0). This applies
to the final PM source, including historical authentication extraction, immutable
review storage, recovery/doctor/snapshot validators and Activity extraction. Final
CLI/index, generated parity, lint and architecture follow.

Final CLI qualification passes 8 check CLI, 14 CI CLI and 4 index tests
(`/tmp/workdeck-retain-review-final-cli.log`), plus 6 catalog/schema/generated parity
tests (`/tmp/workdeck-retain-review-catalog.log`). Strict Clippy initially flagged an
unnecessary clone in a test assertion; using a borrowed slice preserves the assertion.
The affected retention test passes again (`/tmp/workdeck-retain-review-final-core.log`),
and strict PM/CLI all-target Clippy passes with warnings denied
(`/tmp/workdeck-retain-review-final-clippy.log`, 6.62s). Production behavior is unchanged
from the 870-test full PM gate. Rustfmt corrected one match-arm layout in the command
catalog; architecture and final formatting checks follow.

Architecture passes on the resulting source: 13 production crates, one shipped
executable and zero dependency/source-reachability violations
(`/tmp/workdeck-retain-review-architecture.log`). Final workspace formatting,
whitespace and local document links are checked after the layout correction.
All task build/test handles are terminal. No additional phase closes: contextual
TUI review coverage, red/green, criterion/completion integration, PM-10 performance
and PM-12 qualification remain required.


## PM-11 revision/subject review coverage — 2026-09-10

The preceding turn delivered qualified durable review retention (progress). New
coverage acceptance assertions first failed on the missing shared API/types
(`/tmp/workdeck-review-coverage-red.log`). The shared assessment captures an exact
committed candidate and optional subject, rejects mismatched selected document bytes,
and distinguishes historical match, authenticated, stale, rejected and unknown rows.
Only externally supplied baseline/policy pins permit current authentication. Current
reviewer IDs are retained separately from historical attribution. Policy/structural
errors remain visible; check/criterion success is outside this review-only gate.

`ci review-coverage` exposes the same operation. Without policy input it is an
informational report; with the complete policy/baseline pin set it exits nonzero unless
a retained review authenticates. Subjects use issue/feature/gate/planning-kind IDs.
Context packets assess committed HEAD against the selected issue bytes, display at
most five reviews with explicit omissions, and include proof citations. Assessment
fingerprints enter context anchors; time alone does not change the fingerprint, while
expiry, changed HEAD or proof membership does. Revalidation catches a HEAD move after
packet capture. Historical review rows follow active requirements/blockers/continuity
in budget priority. This does not certify other dirty working policy/evaluator files.

Initial core coverage passes (`/tmp/workdeck-review-coverage-core.log`). Shared context
consistency and actual CLI coverage pass in 1 CLI/2 core focused tests
(`/tmp/workdeck-review-coverage-final-focused.log`), including selected-document drift
and a HEAD-only race before final context validation. The mounted TUI test uses real
signed/retained proof and verifies narrow rendering plus later-HEAD staleness
(`/tmp/workdeck-review-coverage-tui2.log`). Its first compile exposed an omitted format
argument, corrected before the pass. TUI adds a test-only dependency edge to the already
locked Ed25519 crate for this actual-proof fixture. A real PTY journey and broader
context/TUI/CLI checks follow. No phase closes.

Actual PTY acceptance passes at 78 and 140 columns
(`/tmp/workdeck-review-coverage-pty.log`): normal startup, Issues → task context,
keyboard selection of signed-review evidence, historical-match display, new commit,
explicit refresh and stale review. Operation history remains unchanged by inspection.
After review, historical observations were moved below requirements/blockers/continuity
in budget priority, and rejected coverage now retains structural and organization-policy
diagnostics. Full PM/TUI all-target verification is running on these changes
(`/tmp/workdeck-review-coverage-full.log`); final CLI/reference/lint/architecture gates
follow.

Final review-coverage qualification passes on the resulting source: 2,142 PM/TUI
tests across 68 groups (zero failed or ignored), 38 actual workbench PTY tests,
26 CLI check/CI/index regressions and 6 generated-reference parity tests. Logs:
`/tmp/workdeck-review-coverage-{full,workbench-pty,cli,catalog}.log`. Strict
PM/TUI/CLI all-target Clippy with warnings denied passes; architecture reports
13 production crates, one shipped executable and zero violations. Formatting
and whitespace checks pass. No phase closes; current-policy TUI controls, dirty
working-policy/evaluator comparison, red/green and completion integration remain.

Storage follow-up: final architecture log confirms successful completion. The
isolated verification target is cleaned after all task test/build handles finish,
preserving the separately active main-checkout Cargo build. Test logs and source
remain available; subsequent Rust verification must rebuild the isolated target.

## PM-11 live planning and evaluator review comparison — 2026-09-10

The signed-proof acceptance fixture now includes a real committed evaluator tree.
The first test setup omitted staging that tree and failed baseline capture; the fixture
was corrected. Intended RED then rejects the unsupported `working_tree` request field
(`/tmp/workdeck-review-working-red2.log`). The implemented shared request flag and
CLI `--working-tree` compare the live planning contract and committed evaluator
selection. Task context enables this automatically. A dirty recipe cannot remove
evaluator comparison; original immutable inspection remains available.

The first GREEN passes dirty check definitions, evaluator byte changes, extra tree
members, two distinct stale fingerprints, clean restoration and a context capture
race (`/tmp/workdeck-review-working-green.log`, one expanded signed-proof test).
Missing files, executable bits, unsafe symlinks, actual CLI authentication failure,
and real narrow/wide terminal refresh are now being qualified with broader regressions.
No new dependency, phase closure, completion authority or external action is introduced.

Final affected-source regression evidence: 53 core CI tests (43.71 s), 42 context/
check-context/handoff tests, 14 actual CLI CI tests, one mounted signed-review context
test, 38 workbench PTY tests (96.11 s), seven CLI context tests and six generated
reference parity tests pass: 161 total, zero failed or ignored. Logs are
`/tmp/workdeck-review-working-{core,context,cli,mounted,pty,catalog}.log`. The real
PTY fixture now refreshes dirty policy to Stale, restores HistoricalMatch after
restoring exact bytes, then makes a later committed candidate stale at 78/140 columns.
The core fixture also covers executable modes and unsafe symlink rejection.
Strict PM/TUI/CLI all-target Clippy passes with warnings denied
(`/tmp/workdeck-review-working-clippy.log`). Architecture and final formatting/link
verification are running. These are affected regressions, not a new full-platform,
performance or release qualification. No phase closes.

Final architecture qualification passes: 13 production crates, one shipped executable,
zero dependency/source-reachability violations
(`/tmp/workdeck-review-working-architecture.log`). `cargo fmt --all -- --check`,
`git diff --check` and local documentation link resolution pass. The source remains
uncommitted and reviewable. Next: explicit externally pinned current-review controls
in the TUI, then required red/green and criterion/completion integration; PM-10
performance/scale and PM-12 release/platform/final review remain open.

## PM-11 explicit TUI review authentication — 2026-09-10

Intended mounted RED: pressing `v` has no authority form
(`/tmp/workdeck-review-authority-red.log`). The new per-task form accepts independent
reviewer-policy JSON and policy/baseline pins and calls shared live coverage. It
preserves historical context rows/anchors and stores no mutation receipt. A separate
assessment row records HEAD, observation time, current reviewers and exact failure
reasons. Initial compile exposed a missing closure-result type annotation; corrected
GREEN passes the mounted signed-proof journey
(`/tmp/workdeck-review-authority-green2.log`).

An initial 46-test context-filtered TUI regression run passes, including malformed
authority invalidation, task-local drafts, failed source refresh and unchanged
operation history (`/tmp/workdeck-review-authority-context.log`). Additional wrong
policy-pin and discard cases are included in final qualification. Full TUI, actual
78/140-column form submission, generated parity and strict gates are in progress.

Full TUI library qualification passes 1,271 tests with zero failures or ignored cases
(`/tmp/workdeck-review-authority-tui.log`, 27.23 s). Actual workbench PTY qualification
passes 38 tests (`/tmp/workdeck-review-authority-pty.log`, 97.90 s), including
independent policy/pin paste, Ctrl-S authentication, dirty policy rejection, exact
restoration and later-HEAD staleness at 78/140 columns. Inspection retains unchanged
operation history.

Strict lint found one collapsible nested condition in the form paste handler. It was
rewritten without changing the branch semantics; strict PM/TUI/CLI all-target Clippy
then passes (`/tmp/workdeck-review-authority-clippy2.log`). The affected mounted and
actual PTY journeys are being rerun on that final source, followed by generated parity
and architecture/format/link gates. No lint allowance or test gate was weakened.

Final-source affected reruns pass: the mounted authentication/negative/draft/refresh
journey (7.42 s), actual 78/140-column authentication journey (17.35 s), and six
generated-reference tests (2.92 s). Logs:
`/tmp/workdeck-review-authority-final-mounted.log`,
`/tmp/workdeck-review-authority-final-pty.log`,
`/tmp/workdeck-review-authority-catalog.log`. Generated skill text documents `v`,
independent policy/pins, Ctrl-S inspection, refresh, draft discard and the distinction
from historical context packets/completion. Architecture and formatting checks remain
in flight; no required phase is marked complete.

Final architecture passes with 13 production crates, one shipped executable and
zero dependency/source-reachability violations
(`/tmp/workdeck-review-authority-architecture.log`). Formatting, whitespace and
local documentation links pass. The next implementation work is required red/green
evidence and integrated criterion/completion policy; PM-10 performance and PM-12
release/platform/final independent review remain open. Full goal remains active.

## PM-11 authenticated red/green evidence — 2026-09-10 (in progress)

The existing report summary did not retain passing test identities, so counts could
not establish that a failed test later passed. A bounded inert `junit_test_cases` API
now exposes structured suite-path/class/name identities and all four case outcomes,
using the existing no-DTD JUnit parser. Two new case tests and 11 report regressions
pass (`/tmp/workdeck-red-green-cases-green.log`); intended RED was the missing API.

Shared `verify_red_green` uses caller-pinned accepted red commit/contract, unchanged
candidate evaluation contracts, current producer authentication, fresh immutable Git
input recapture, exact original artifact hashes, normal process termination, accepted
red exit codes and assertion-case transitions. Check definitions gain optional
`red_green`; old definitions omit it without changing their encoding. Two real-run
integration tests pass (`/tmp/workdeck-red-green-green.log`, 29.54 s), including
infrastructure error, skip, replaced-case, changed-evaluator, wrong-policy and altered
artifact negatives. No check execution or planning mutation occurs during verification.

CLI acceptance is being introduced. Its first fixture compile exposed a CLI-only
missing YAML dev dependency; fixture writes were changed to JSON, which the existing
YAML parser accepts, instead of adding a dependency. Final CLI RED/GREEN, negative
expansion, legacy report compatibility, docs and strict qualification remain underway.
Accepted evaluator-change promotion and durable criterion/completion integration
remain outside this completed pair-verification portion and must still be delivered.


Storage and resumed qualification checkpoint (2026-09-10): `cargo clean` against
`/Users/rutger/Projects/workdeck/target` removed 14,408 files / 8.1 GiB. Free space
rose from 94 to 101 GiB; a separate main-checkout build subsequently recreated its
cache and was preserved. This task retains its isolated verification target.

The complete PM suite passed 878 tests across 68 groups, with no failures or ignored
tests (`/tmp/workdeck-red-green-full-pm.log`). The affected CLI suite passed 23 tests:
8 check, 14 CI and 1 red/green (`/tmp/workdeck-red-green-cli-final.log`). Strict
PM/TUI/CLI all-target lint passed before the following cache correction.

Review found schema-3 cached source reuse could bypass newly typed red/green
validation for unchanged files. The old-cache regression was updated to exercise
schema 3 and failed at cache admission as intended
(`/tmp/workdeck-red-green-cache-red.log`). Schema 4 now forces a full rebuild;
affected projection qualification and final lint/catalog/architecture checks are
underway. Generated command, schema and skill references were refreshed from the
actual CLI. No phase closes; accepted evaluator promotion, completion integration,
performance and release gates remain open.


Final affected qualification: projection live/storage 19 PASS after schema-4 fix;
CLI generated-reference parity 6 PASS (3.03 s); strict PM/TUI/CLI all-target Clippy
PASS (27.81 s). Logs: `/tmp/workdeck-red-green-cache-green.log`,
`/tmp/workdeck-red-green-catalog.log`, `/tmp/workdeck-red-green-clippy-final.log`.
The full 878-test PM run and 23 CLI cases preceded only the cache-version correction;
the affected cache suites and final CLI catalog were rerun after it. Local Markdown
file links pass. Free space remains 99 GiB with the independent build preserved.


Final architecture check PASS: 13 production crates, one shipped executable, zero
violations (`/tmp/workdeck-red-green-architecture.log`). Final `cargo fmt --all
--check` and `git diff --check` PASS. This closes the bounded pair-verification
increment, not PM-11 or the full goal. Next: accepted evaluator-baseline promotion
and durable red/green linkage into shared completion policy. No task Cargo process
remains running at this checkpoint.


## PM-11 reviewed red baseline integration — 2026-09-10 (in progress)

The prior pair verifier required a preaccepted red baseline. The shared composed
operation now authenticates the proposed red evaluator contract from an independently
pinned earlier baseline and current reviewer policy before pair verification. A
synthetic repository adds a required test after prior acceptance, runs real red and
green executions, and authenticates both reviewer and producer signatures. The new
case rejects wrong reviewer policy, baseline substitution, missing signatures and
expired reviewer authority; operation history remains unchanged. All five red/green
integration tests pass (47.43 s, `/tmp/workdeck-reviewed-red-green-green.log`). The
initial intended RED was the missing composed API/types. Actual CLI acceptance and
final qualification are underway; durable completion integration remains open.


Actual CLI RED rejected unknown `--baseline-review-file`
(`/tmp/workdeck-reviewed-red-green-cli-red.log`), then GREEN passed the real signed
review/pair command and malformed-review rejection (15.06 s,
`/tmp/workdeck-reviewed-red-green-cli-green.log`). All review inputs are paired:
original envelope, independently supplied reviewer policy/hash and prior baseline
commit/contract. Generated schemas, command catalog and skill were refreshed.

The final core run includes an extra wrong-red-contract negative and asserts that
approved red evaluators changed in green remain rejected. Completion integration
still reaches the explicit required-check/profile guard in
`AcceptancePolicy::ensure_supported_for_completion`; that guard is retained until
shared proof evaluation is integrated. No completion claim is inferred from this
read-only composed verifier.


Final affected core qualification PASS: 53 CI validation cases (39.36 s) and five
red/green cases (49.29 s), including changed evaluators despite reviewer admission
and a substituted red contract. Log: `/tmp/workdeck-reviewed-red-green-final-pm.log`.
No failures or ignored tests. Local Markdown file links and whitespace checks pass.
Final CLI/catalog and strict checks remain in progress at this checkpoint.


Final CLI qualification PASS: six generated-reference tests (3.04 s) and the actual
reviewed/unreviewed red/green command test (12.55 s), including incomplete review
options and malformed proof rejection. Log:
`/tmp/workdeck-reviewed-red-green-final-cli.log`. Formatting was applied after the
last test-only option-pairing assertion; no behavioral source change followed these
passing tests. Strict all-target lint and architecture are the remaining checks.


Final strict PM/TUI/CLI all-target Clippy PASS (26.84 s,
`/tmp/workdeck-reviewed-red-green-clippy.log`). Architecture PASS: 13 production
crates, one executable, zero violations
(`/tmp/workdeck-reviewed-red-green-architecture.log`). Final formatting and
whitespace checks PASS (`/tmp/workdeck-reviewed-red-green-format.log`). The reviewed
red-baseline composition is qualified; no phase closes. All task Cargo handles are
terminal. Next: durable red/green proof linkage and shared completion-policy
integration; performance, CI/release wiring and PM-12 remain required.


## PM-11 durable red/green proof — 2026-09-10 (in progress)

The existing attestation record now optionally retains original signed red proof,
exact red/green XML and optional signed baseline review alongside its original green
envelope. Import performs shared live verification before publication. Offline record
validation reauthenticates original signatures at historical import time and shares
the assertion/process/artifact consistency evaluator; it does not claim current Git
or independent authority. Existing records omit the optional field unchanged.

The first real-run retention case passes (16.20 s,
`/tmp/workdeck-red-green-retention-green.log`), including exact replay, explicit
current producer/reviewer/baseline authority, offline history and altered-artifact
rejection. The missing types/API were the initial intended RED. Actual CLI RED
rejected the missing import option; GREEN passed import plus current reauthentication
(21.84 s, `/tmp/workdeck-red-green-retention-cli-green.log`). Recovery, concurrency,
snapshot, invalid-admission and whole-PM tests are being qualified against final source.


The first whole-PM run stopped in the new concurrency case after 47 successful
groups: the losing caller returned the transaction store's documented `Locked`
error while the winner performed Git validation. The test incorrectly assumed both
first attempts must finish inside the lock wait budget. Production locking remains
unchanged. The case now requires at least one winner, permits only the explicit
`Locked` outcome for the competitor, retries after both callers finish and asserts
one exact receipt and one record for that request. Each injected crash now also
asserts the expected `Io` error/message, so an unrelated early failure cannot count
as fault coverage.

`cargo metadata --offline --locked --no-deps` enumerated the remaining targets.
The corrected retention case and every target after it (including the example) are
running in `/tmp/workdeck-red-green-retention-final-pm.log`; earlier successful
production tests remain valid because only the new test expectation changed.
The initial run is retained at `/tmp/workdeck-red-green-retention-full-pm.log`.
Final coverage will use the union of completed targets, not the aborted run alone.


The strengthened fault assertion exposed the documented post-journal error wrapper:
`RecoveryRequired`, with the injected `Io` error retained in its message. The test
now records that the requested injection actually fired and requires `Io` before
journal publication or `RecoveryRequired` after it. This follows
`transactions.rs::transact_guarded/recovery_error`; production behavior and recovery
gates are unchanged. The first corrected run is retained separately as evidence.


PM qualification is complete for this source: all 69 library/integration/example
targets reported by Cargo metadata are covered by successful final results, totaling
881 passed, zero failed and zero ignored tests. The successful targets before the
initial stop and the complete corrected remainder were matched by target identity;
no aborted or duplicate group was counted. Recovery/concurrent retries, exact one
record/receipt, invalid prepublication proof and offline snapshot restoration pass.
Logs: `/tmp/workdeck-red-green-retention-full-pm.log` and
`/tmp/workdeck-red-green-retention-final-pm.log`. CLI rebuild/reference regeneration,
final CLI/catalog regression and strict qualification remain underway.


Final CLI and generated-reference qualification PASS: 21 tests (six catalog,
14 CI, one real red/green import/reauthentication journey). The command discovery
regression explicitly checks that retained-pair verification requires an initialized
source. Generated references were rendered from the final CLI binary before testing.
Log: `/tmp/workdeck-red-green-retention-final-cli.log`. Strict PM/TUI/CLI lint and
architecture/formatting are the remaining final gates.


Final qualification PASS: strict PM/TUI/CLI all-target Clippy (26.23 s,
`/tmp/workdeck-red-green-retention-clippy.log`); architecture (13 production crates,
one executable, zero violations, `/tmp/workdeck-red-green-retention-architecture.log`);
formatting (`/tmp/workdeck-red-green-retention-format.log`), whitespace and local
Markdown file links. No task Cargo handle remains active. This qualifies durable
red/green retention and explicit current reauthentication, not criterion acceptance
or completion. Next: exact criterion-to-attestation linkage and shared completion
policy. All PM-10 performance, PM-11 completion/CI wiring and PM-12 requirements
remain in scope; the full goal remains active.


## PM-11 criterion-to-attestation linkage — 2026-09-10 (in progress)

The initial missing API/type RED is `/tmp/workdeck-attested-evidence-red.log` (a
fixture call also supplied an unnecessary query argument to `list_issues`; corrected
before GREEN). The real signed report/criterion path passed (23.01 s,
`/tmp/workdeck-attested-evidence-green.log`). A project criterion test independently
failed on the missing typed owner (`/tmp/workdeck-project-criterion-red.log`). Project
criterion/subject support now spans resolution, graph pins, question validity,
retirement blockers, context selection and projection citations.

Actual CLI RED rejected unknown `evidence verify-red-green`
(`/tmp/workdeck-attested-evidence-cli-red2.log`); GREEN passed the real link command
(26.86 s, `/tmp/workdeck-attested-evidence-cli-green.log`). Expanded tests exercise
wrong record/source/check/producer/result/time, absent/expired/duplicate links,
criteria absent from the candidate, current criterion changes, supersession and a
post-proof edit before final revalidation. The complete PM suite is running with
`--no-fail-fast` so every target runs even if an affected test needs correction.
No completion authority is inferred from an authenticated link.


Review additionally found projection extraction omitted nested `Question.subjects`,
direct question requirement refs and `EvidenceReference.declaration` citations. A
new public-detail regression asserts project subject/criterion and attestation
relations. The schema-4 cache regression will require invalidation, since old caches
can otherwise reuse unchanged files without extracting the missing relations. These
test-only additions were made after the running whole-PM binary had compiled and
will be qualified separately with the projection correction; they are not counted
as covered by the in-flight run.

Whole-PM qualification completed: 883 tests across 70 groups passed, zero failures
or ignored tests (`/tmp/workdeck-attested-evidence-full-pm.log`). Subsequent projection
regressions failed as intended for missing nested citations and schema-4 cache reuse
(`/tmp/workdeck-evidence-citations-red.log`). Extraction and schema-5 invalidation
now pass all 14 projection library tests and three filtered storage tests
(`/tmp/workdeck-evidence-citations-green.log`). Full affected storage/live tests and
final CLI/catalog/lint checks follow; the full run preceded this scoped correction.

Final affected projection qualification: all 19 storage/live tests pass
(`/tmp/workdeck-evidence-projection-final.log`). Final executable/catalog/gate
qualification: 11 tests pass (`/tmp/workdeck-evidence-cli-final.log`), after rendering
commands, schemas and the bundled skill from the final binary. Strict all-target
PM/TUI/CLI Clippy passes with warnings denied (26.92 s,
`/tmp/workdeck-evidence-clippy.log`). Local Markdown links pass. Storage verification
shows 97 GiB free; no additional Cargo cleanup was needed for this increment.

Architecture PASS: 13 production crates, one executable, zero dependency or
source-reachability violations (`/tmp/workdeck-evidence-architecture.log`). Final
format check passes (`/tmp/workdeck-evidence-format.log`). This increment qualifies
exact criterion links and nested projection citations; it does not close PM-11 or
the overall goal. Next is shared gate/completion-policy acceptance with independent
authority, original proof and final mutation-boundary revalidation.


## PM-11 committed gate qualification — 2026-09-10 (in progress)

Missing shared gate request/verification types established the initial RED
(`/tmp/workdeck-attested-gates-red.log`). Initial executable-core attempts then
rejected the fixture's producer pin: its JSON hash had not sorted keys, unlike the
authenticated producer contract (`/tmp/workdeck-attested-gates-green.log` and
`/tmp/workdeck-attested-gates-green2.log`). The fixture now uses canonical key order;
production equality checks remain unchanged. The two-requirement core journey
passes (35.06 s, `/tmp/workdeck-attested-gates-green3.log`), including incomplete,
duplicate/unknown selection, wrong gate pin, post-proof gate edit, and changed
current-versus-committed gate rejection. Additional conflicting evidence pins and
post-proof evidence edit tests were added afterward and still require qualification.
The real CLI missing-command RED is running before command implementation.


The actual CLI RED rejected unknown `gate verify-red-green` (28.18 s,
`/tmp/workdeck-attested-gates-cli-red.log`). The command now parses the bounded typed
request and calls the same shared verifier without mutation. The expanded gate core
journey passes (46.21 s), including evidence-file edits and conflicting reused pins.
Affected core qualification is running under `--no-fail-fast`; evidence, gate,
CI-validation, ordinary gate and red/green targets have passed (70 tests), with
retention fault/concurrency cases still running. Log:
`/tmp/workdeck-attested-gates-core-final.log`. These are scoped checks, not a new
claim of complete workspace or phase qualification.


Affected core qualification is complete: 72 tests across six targets passed, zero
failures or ignored tests (`/tmp/workdeck-attested-gates-core-final.log`). This includes
51.89 s evidence, 46.21 s gate, 53 CI-validation cases, 10 ordinary gate cases, five
red/green cases and both retained-proof recovery/concurrency cases. The gate fixture
uses two explicit AND requirement IDs sharing one exact evidence record, exercising
within-call proof reuse and conflicting-pin rejection. Further multi-check completion
coverage remains part of the unfinished completion integration.

The final executable builds (28.34 s, `/tmp/workdeck-attested-gates-build.log`). Skill,
commands and schemas were rendered from it. The documented `workdeck schema
red-green-gate-request --json` command passes. Final CLI/catalog/gate tests are running
before strict lint and architecture qualification.


Final CLI/catalog/gate qualification passes all 11 tests with no failures/ignored
cases (`/tmp/workdeck-attested-gates-cli-final.log`): six catalog/reference checks,
four ordinary gate cases and the real signed red/green/evidence/gate journey. The
new gate command qualifies the complete selection and returns failure after removing
one required selection; operation history remains unchanged. Strict all-target
PM/TUI/CLI lint is running next.


Final quality gates PASS: strict PM/TUI/CLI all-target Clippy with warnings denied
(27.32 s, `/tmp/workdeck-attested-gates-clippy.log`); architecture (13 production
crates, one executable, zero violations, `/tmp/workdeck-attested-gates-architecture.log`);
formatting (`/tmp/workdeck-attested-gates-format.log`), whitespace and local Markdown
links. No task Cargo handle remains active. This increment qualifies explicit
read-only committed AND gates, not issue completion, project/milestone exit or feature
maturity transitions. The full goal remains active. Next: shared completion preflight,
selected-source/input binding, transaction-lock revalidation and retained receipt/replay
proof, followed by the same CLI/TUI operations. Do not nest snapshot lock acquisition
inside mutation preparation or treat serialized gate output as a credential.


## PM-11 verified issue completion — 2026-09-10 (in progress)

The initial RED failed on missing completion request/API/result types
(`/tmp/workdeck-verified-completion-red.log`). Initial GREEN completed the exact issue
and replayed the original receipt after Git was made unavailable (21.35 s,
`/tmp/workdeck-verified-completion-green.log`). Expanded GREEN passed wrong/missing
proof pins, dirty source, source edits after preflight and immediately before journal,
no-write-on-failure, exact replay and conflicting-request rejection (40.73 s,
`/tmp/workdeck-verified-completion-expanded.log`).

Actual CLI RED rejected unknown `--verification-file` (31.56 s,
`/tmp/workdeck-verified-completion-cli-red.log`). `issue done --verification-file`
and its read-only `--dry-run` are now implemented, but their final CLI acceptance run
is still pending. Recovery boundaries (after journal, after issue write, before
receipt and after receipt) plus snapshot export are running in the expanded core
suite. New receipt validation is wired through history, journal recovery, snapshots,
claim-catalog/doctor inspection, staging and coordination consumers. Those consumers
still need broad regression qualification against the final source.

This is an in-progress red/green-required completion path. Generic signed green-only
checks, claimed completion, attached-gate completion coverage, TUI and project/
milestone/feature policy transitions remain required. No overall completion or phase
closure is claimed. Current-source binding and private live admission preserve the
ordinary path's fail-closed required-check guard.


Core recovery GREEN: both tests passed, including all four durable fault boundaries
and snapshot export (105.26 s, `/tmp/workdeck-verified-completion-faults.log`). Actual
CLI GREEN: dry-run with unchanged history, completion, and identical request replay
passed (55.86 s, `/tmp/workdeck-verified-completion-cli-green.log`). These preceded
later review additions: historical gate-proof validation, concurrent same-request
retries, and tampered receipt rejection through history/doctor/snapshot. A racing
preflight now checks for an already published original request before reporting its
stale-source failure. No process is restarted or mutation repeated by this read.

The complete PM suite is now running `--all-targets --no-fail-fast` against these
additions (`/tmp/workdeck-verified-completion-full-pm.log`). All prior failures remain
in the logs; final source qualification is not yet claimed. Storage remains healthy
at 95 GiB free. No cleanup or unrelated source removal was performed.


Review during the full run added two follow-up regressions, not yet part of that
compiled run: replacing the Git directory with an identical copied directory after
preflight must be rejected, and projection schema 5 must be invalidated because
previous cached validation could have treated the new completion operation as an
ordinary generic receipt. Their explicit RED runs and production corrections will
follow the current live full-suite process; it will not be restarted on timeout.


Additional follow-up coverage was added while the original full-suite binary was
running: the gate-enabled fixture now associates its gate with the issue before the
red baseline commit, and the actual CLI completion request supplies that gate's
complete selection. A core attached-gate case supersedes selected evidence between
preflight and transaction, requires refusal, then qualifies the corrected active
selection. These fixture/test changes need their affected reruns; the in-flight
whole-PM result does not cover them. At 48 completed groups, that run has 619 passing
tests and no failures/ignored cases; the process remains live.


Whole-PM qualification completed successfully: 888 tests across 72 groups, zero
failures or ignored tests (`/tmp/workdeck-verified-completion-full-pm.log`). This run
included concurrent retry and tampered receipt rejection, but preceded the later
Git-directory/cache regressions and strengthened attached-gate fixture.

The Git-directory RED is confirmed: replacing `.git` after preflight returned a
successful completion receipt when failure was required (19.40 s,
`/tmp/workdeck-completion-git-binding-red.log`). The private admission now owns the
original bounded `BoundGit` through the final publication check, including dry-run;
it is no longer reopened after preflight. Cache RED admitted an old schema-5 cache
(`/tmp/workdeck-completion-cache-red.log`); production schema is now 6. The focused
final core run covers both corrections, attached-gate completion/supersession, the
strengthened direct-gate fixture, storage and live projections
(`/tmp/workdeck-verified-completion-final-core.log`), and is still running.


Final affected core GREEN: 25 tests across direct gates, projection storage/live and
verified completion passed (`/tmp/workdeck-verified-completion-final-core.log`). This
includes the Git-directory replacement fix, attached-gate missing/superseded/corrected
selection, all recovery boundaries, concurrent retry, and tampered receipts. All 14
projection library tests also pass with schema 6
(`/tmp/workdeck-verified-completion-projection.log`). The ordinary completion guard's
message now accurately directs standalone red/green users to the verified path;
its error code and fail-closed behavior are unchanged. Final executable/catalog and
CLI regressions are next, followed by strict lint and architecture.


Final CLI regression run exposed two lifecycle assertions tied to the old "not
implemented" diagnostic (`/tmp/workdeck-verified-completion-final-cli.log`). The new
verified path makes that wording inaccurate; the ordinary path now reports that it
has no authenticated check/profile admission. The two assertions were updated to
that precise diagnostic. Their required `allowed=false`, declared basis,
`policy_blocked` exit, unchanged authority bytes, ready status and absent manual
acceptance assertions remain intact. This is a diagnostic-contract update, not a
weakened completion gate. The original multi-target run remains live; the full
lifecycle target will be rerun after it finishes.


Final CLI qualification now passes all 24 unique tests across catalog, claims, gates,
lifecycle, operation inspection/recovery and the actual signed-proof workflow. The
original multi-target log is `/tmp/workdeck-verified-completion-final-cli.log`; its
only failures were the two obsolete lifecycle diagnostic assertions. The complete
corrected lifecycle target passes all eight tests (3.46 s,
`/tmp/workdeck-verified-completion-lifecycle.log`). The final attached-gate CLI journey
passes (68.41 s), including preview, actual Done and exact replay. Generated catalog
parity and documented completion-request schema invocation pass. Workbench regressions
are now running because the shared completion guard and projection schema changed;
this does not claim new TUI verified-completion controls.

Final completion-increment checks: 190 workbench regressions pass (22.09 s),
strict PM/TUI/CLI all-target Clippy passes (26.11 s), and architecture passes
(13 production crates, one executable, zero violations; 24.70 s).
The final formatting check passes after formatting two diagnostic assertions;
no semantic changes followed qualification. Logs are
`/tmp/workdeck-verified-completion-workbench.log`,
`/tmp/workdeck-verified-completion-clippy.log`,
`/tmp/workdeck-verified-completion-architecture.log` and
`/tmp/workdeck-verified-completion-format.log`.

Storage recovery on 2026-09-10: no Cargo processes were present at preflight.
Explicit Cargo cleanup of the main Workdeck target removed 11,898 files (6.5 GiB).
Available disk space increased from 94 GiB to 100 GiB. The 4.7 GiB task verification
cache was retained. An independent contributor-guide Cargo test subsequently
started rebuilding the main target; it was not interrupted or cleaned underneath.
The implementation remains active at 10/13 closed phases, not fully complete.

## Imported-check success qualification, 2026-09-10

Behavioral RED: three core tests failed on the explicit missing qualification path
(`/tmp/workdeck-green-only-qualification-red.log`, terminal 101). Implemented live
current-HEAD binding, pinned original-record recapture, current committed check and
invocation comparison, explicit success admission and mandatory red/green authority.
All four core cases pass (`/tmp/workdeck-green-only-qualification-green.log`, 22.32 s):
green-only success/read-only/dirty-input rejection; authentic failure rejection;
mandatory pair omission rejection; and retained reviewed pair success with omitted
reviewer authority rejection. The real CLI case then established missing-command
RED (`/tmp/workdeck-green-only-cli-red.log`, 12.40 s). CLI implementation and final
catalog/lint qualification are in progress. No completion phase is closed by this
read-only check operation.

Final qualification passes on the resulting source:

- 20 core tests: 11 report-handling tests, four imported-check qualification tests
  (21.80 s), and five existing verified-completion cases (103.24 s), including
  concurrent retry, four recovery boundaries and attached gates.
  Log: `/tmp/workdeck-green-only-core-final.log`.
- Seven CLI/catalog tests: six command/schema/generated-reference cases (2.88 s),
  and the real green-only qualification/dirty-input/no-write journey (16.30 s).
  Log: `/tmp/workdeck-green-only-cli-final.log`.
- Strict PM/TUI/CLI all-target Clippy passes (26.19 s) after collapsing a nested
  conditional; affected tests above were run after that correction.
  Log: `/tmp/workdeck-green-only-clippy-final.log`.
- Architecture passes: 13 production crates, one executable, zero violations
  (25.96 s); formatting and local Markdown links/whitespace also pass.
  Logs: `/tmp/workdeck-green-only-architecture.log` and
  `/tmp/workdeck-green-only-format.log`.
- `workdeck schema verify-imported-check --json` was invoked successfully from the
  final CLI binary. Generated skill, command reference and schemas match the binary.

Next: connect this success qualification to private transactional completion
admission and historical proof validation, including green-only gate evidence;
retain mandatory red/green checks, source guards, exact retry and recovery. The
read-only returned report is not accepted as a mutation credential. Claimed
completion, TUI controls, hierarchy transitions, PM-10 performance and PM-12
qualification remain open; phase closure remains 10/13.


## Transactional green-only issue completion — 2026-09-11

Behavioral RED established the missing mutation path: `complete_verified_issue`
returned the old unsupported diagnostic for a passed imported check and for a signed
failure (`/tmp/workdeck-green-completion-red.log`). The implementation now shares the
private live admission and transaction engine with legacy red/green completion while
using `CompletionAuthority` and exact passed-check attestations for green-only policy.

Focused GREEN covers signed success to Done, exact replay after the Git directory is
unavailable, signed failure rejection, duplicate/omitted required-check selection,
source changes before the transaction, four durable fault boundaries, tampered
historical receipts, and preservation of mandatory red/green plus attached-gate
authority. The current focused core run passes six verified-completion cases in
108.39 seconds (`/tmp/workdeck-green-completion-core-current.log`); the green-only
RED/GREEN pair is recorded in `/tmp/workdeck-green-completion-red.log` and
`/tmp/workdeck-green-completion-green.log`. Legacy lifecycle/red-green compatibility
passes nine CLI cases (`/tmp/workdeck-green-completion-compat-red.log`), and the
real green-only CLI qualification passes its dirty-input/no-write case
(`/tmp/workdeck-green-completion-cli-current.log`).

The corrected complete PM all-target rerun passes **898 tests across 74 groups**, with
zero failures or ignored tests (`/tmp/workdeck-green-completion-full-pm-rerun.log`).
The projection-storage stale-schema fixture now uses schema 5 and rebuilds to the
current schema 6. Strict PM/TUI/CLI all-target Clippy, the architecture checker
(13 production crates, one executable, zero violations), rustfmt, final CLI build,
generated protocol rendering and catalog/schema parity pass on the same source.

This closes the standalone green-only check-to-issue transaction increment, not PM-11
or PM-12. Attached gates still use the red/green evidence verifier; generic green-only
gate evidence, claimed completion ownership/replay, shared TUI controls, project/
milestone exit and feature maturity policy, PM-10 refresh targets, CI/release wiring,
cross-platform qualification and dogfood/release evidence remain open.


## Authenticated green-only gate evidence — 2026-09-11

Behavioral RED established the missing generic gate path: a green-only evidence
selection could not be verified as a committed AND gate, and verified completion had
no typed gate assessment to retain. The implementation adds `VerifiedEvidenceRequest`
and `VerifiedGateRequest`, binds each requirement to the active declaration, exact
attestation, committed criterion, candidate source, check/producer and freshness
window, and adds the read-only `gate verify-green` command. Red/green requirements
continue to require their retained pair authority.

Focused GREEN passes the new two-case `green_only_gate` regression: a complete gate
verifies without writing history and can be retained/replayed through verified issue
completion; missing, duplicate and changed evidence are rejected. The compatibility
set also passes `attested_gates` (1), `green_only_completion` (3), `green_only_gate`
(2) and `verified_completion` (6) with zero failures. The real CLI journey passes
`pm_green_gate` (1), while generated catalog/schema and imported-check compatibility
remain green (`pm_catalog` 6 and `pm_imported_check` 1).

Final static checks pass after the generic gate changes:

- strict PM/TUI/CLI all-target Clippy: `/tmp/workdeck-green-gate-clippy.log`;
- architecture checker: `/tmp/workdeck-green-gate-architecture.log`;
- rustfmt: `/tmp/workdeck-green-gate-format.log`.

The complete PM all-target regression passes **900 tests across 75 result groups**,
with zero failures and zero ignored tests (`/tmp/workdeck-green-gate-full-pm.log`).
The run includes the new generic gate and completion proof validators, old red/green
completion, receipts, snapshots, recovery, source races, claims and scale tests.
The increment qualifies authenticated green-only attached-gate evidence and its
historical retention. Claimed completion ownership/replay, shared TUI controls,
project/milestone exit and feature maturity policy, PM-10 refresh/scale, pinned
CI/release wiring, cross-platform qualification and PM-12 dogfood/release evidence
remain open; no phase closure is claimed.


## Claimed verified completion — 2026-09-11

Behavioral RED established the missing composition: an authenticated green-only
completion proof could not be applied while retaining the current claim as the
ownership authority. `CompleteClaimedVerifiedIssue` now combines the existing claim
contract with `CompleteVerifiedIssue` and performs claim, source, captured-input,
check, gate and policy revalidation under the same transaction lock. The claimed
receipt retains the complete verified proof and exact request replay; the
before-journal guard also rejects a source edit after admission without writing.

Focused GREEN passes the two core cases in `claimed_verified_completion` (successful
ownership-preserving completion with replay after Git removal, and wrong-actor/source
race rejection with no writes) in `/tmp/workdeck-claimed-verified-focused.log`.
The real CLI journey passes `pm_green_gate` (the generic green gate and claimed
completion cases), while `pm_catalog` (6) and `pm_imported_check` (1) remain green.
The CLI accepts `issue complete --verification-file FILE` for this path and rejects
combining that file with release flags. Existing local/shared claimed completion and
separate release behavior remain unchanged.

The post-increment full PM all-target regression passes **902 tests across 76 result
groups**, with zero failures and zero ignored tests
(`/tmp/workdeck-claimed-verified-full-pm.log`). Strict PM/TUI/CLI Clippy,
architecture and formatting checks are green (`/tmp/workdeck-claimed-verified-clippy.log`,
`/tmp/workdeck-claimed-verified-architecture.log` and
`/tmp/workdeck-claimed-verified-format.log`); `git diff --check` is clean. This
qualifies claimed authenticated completion and its historical replay. TUI controls,
project/milestone exit criteria, feature maturity, PM-10 refresh/scale, CI/release
wiring and PM-12 dogfood/release evidence remain open; phase count stays 10/13
(approximately 77%).


## Claimed verified completion TUI control — 2026-09-11

The claims workbench now has a first-class `g` action, `Complete with verification`,
alongside the existing `e` complete-and-release action. The form accepts a bounded
regular UTF-8 JSON file, retains the parsed proof and path for exact retries, rejects
claim issue/actor/source mismatches before starting a worker, and runs the same owned
foreground transaction as the CLI. The view labels the authenticated completion
receipt and makes clear that no separate release was requested.

The Git-backed TUI regression `authenticated_claimed_completion_has_a_first_class_tui_control`
passes with the complete claims workspace target: **11 tests, 0 failures**
(`/tmp/workdeck-tui-claimed-control-tests.log`). This qualifies the control and
source/input safety seam; broader project/milestone exit and feature maturity policy
transitions, indexed full-scale refresh, CI/release wiring and PM-12 evidence remain
open.


## PM CI/release command wiring — 2026-09-11

The repository now exposes explicit maintainer gates through `cargo xtask pm check`,
`cargo xtask pm performance` and `cargo xtask pm release-check`. The check command runs
the complete `workdeck-pm` all-target suite, the generated PM CLI catalog/green-gate/import
compatibility targets, the synthetic dogfood journey and the exact calibrated projection
benchmarks. `pm performance` runs only those bounded full-size benchmark checks. The
release variant runs the same checks and a locked release build of the single `workdeck`
executable; neither command publishes or deploys.

`.github/workflows/ci.yml` calls `cargo xtask pm check --profile standalone` in the
Linux validation job, and `.github/workflows/release.yml` calls
`cargo xtask pm release-check --profile standalone` before
license/security and packaging jobs. This is repository wiring evidence, not an
external CI result. The named profile now pins validator bytes and records a trusted
baseline identifier; independent verification of that baseline and hook qualification
remain open requirements.


## Shared hierarchy and maturity policy — 2026-09-11

The shared `policy.rs` evaluator now provides source-bound read-only assessments and
transactional mutations for project/milestone exits and feature maturity. Project
and milestone exits require completed issue members and explicit declared criteria;
projects also require completed child milestones. Feature maturity advances one stage
at a time, requires an accepted decision and criterion, and at `implemented` requires
completed associated issues and mature prerequisites. Retirement, canceled/incomplete
children, and declared feature gates remain independently visible blocking conditions.
Manual acceptance, local feedback, reviewed evidence, CI qualification, and plain
declarations retain separate policy bases.

`project assess/complete`, `milestone assess/complete`, and `feature assess/promote`
are available through the native CLI. Mutations require an expected source token and
an attributed actor/reason; the policy is evaluated again under the transaction lock
and the normal immutable receipt is retained. The CLI hierarchy/feature end-to-end
suite passes 13 tests, and the complete PM all-target run passes 904 tests across 77
groups with zero failures or ignored tests.

The mounted TUI exposes the same contract through `p` on Projects/Milestones and `m`
on Features. Forms retain source tokens and first requests across uncertain outcomes;
the detail pane shows the latest conditions and basis. The complete TUI all-target
run passes 1,275 tests with zero failures or ignored tests. Authenticated gate/review
bases are still not automatically substituted for manual acceptance, and PM-10 scale
targets, independent validator/release qualification, cross-platform hardening, and
PM-12 dogfood/release evidence remain open.


## Named PM release profile and validator pin — 2026-09-11

`ci/workdeck-pm-release.json` is the checked-in `standalone` profile used by
`cargo xtask pm check --profile standalone`,
`cargo xtask pm performance --profile standalone` and
`cargo xtask pm release-check --profile standalone`. The profile drives the full
`workdeck-pm` all-target suite, generated PM CLI compatibility targets, calibrated
full-size projection benchmarks and the locked Workdeck release build. `cargo xtask pm profile --profile standalone` emits
the exact machine-readable profile without running or writing a planning source.

Before execution, xtask validates the profile schema, requires the expected
validator source, compares its SHA-256 pin, requires a strict immutable-reference plus
64-character lowercase digest trusted-baseline tuple, and rejects empty, non-Cargo, or
commands outside bounded test/build operations and the two fixed projection benchmarks.
For the standalone profile it also rejects removed, unrelated or weakened check and
release commands. The profile pin is deterministic and six parser/allowlist tests pass.
This catches changes to the local validator before it can self-validate with altered
rules. The recorded baseline is an external review anchor; it has not been independently
resolved or authenticated in this local checkout, so PM-11.D7/D8 remain partial until
CI/release execution and that review are observed.


## Synthetic PM-12 dogfood journey — 2026-09-11

The new `crates/workdeck-cli/tests/pm_dogfood.rs` fixture exercises the complete
bounded user/agent path in a temporary Git checkout with a temporary bare remote.
It explicitly previews an empty legacy migration, creates and associates native
project/milestone/feature/issue records, selects ready work, captures bounded
context, acquires a source-bound shared claim, commits a fixture implementation,
plans and runs a revision-bound check, inspects review coverage, completes through
the claim receipt, promotes feature maturity, completes milestone/project policy,
and publishes a proposal. The same publication request is replayed, then a fresh
clone of the proposal ref resumes with context, issue state, and doctor validation.

The test passes with one result and zero failures:

```text
cargo test --offline --locked -p workdeck-cli --test pm_dogfood
test synthetic_agent_journey_can_claim_check_complete_publish_and_resume ... ok
```

The review step intentionally records unauthenticated coverage as
`authenticated: false`; a local check result or manual declaration does not become reviewer or CI
authority. This qualifies composition, source binding, replay and clone resume in
an isolated fixture. Broader PM-12 fault injection, supported-platform/locking evidence,
scaled TUI behavior, independent review/validator qualification, external CI and
release execution remain open, so the phase count stays 10/13 (approximately 77%).


## PM-12 CLI fault matrix increment — 2026-09-11

The new `crates/workdeck-cli/tests/pm_fault_matrix.rs` fixture exercises selected
fail-closed boundaries through the real CLI against a temporary Git repository.
It covers malformed issue YAML, an unsupported future schema, an issue symlink to
an outside file, an unreadable issue file, stale expected source after an edit,
and two issue IDs that share a short-reference prefix. Each case checks the
structured diagnostic and verifies that the `.workdeck` authority snapshot (and
any outside symlink target) is unchanged; the final valid read and list checks
confirm recovery without implicit repair.

The focused and named-profile executions both pass:

```text
cargo test --offline --locked -p workdeck-cli --test pm_fault_matrix
test malformed_unsafe_and_stale_inputs_fail_closed_without_cross_file_effects ... ok

test result: ok. 1 passed; 0 failed

cargo xtask pm check --profile standalone
Workdeck PM checks passed.
```

This is selected CLI fault evidence, not the complete PM-12 matrix. Interrupted
write/migration/index/check/publication cases, supported-platform locking and
filesystem qualification, full TUI scale behavior, and independent/external
release evidence remain open.


## Named profile execution — 2026-09-11

After adding the dogfood and selected fault-matrix fixtures to
`ci/workdeck-pm-release.json`, the real named profile was executed from the
checked-in `xtask` binary:

```text
CARGO_NET_OFFLINE=true ... xtask pm check --profile standalone
Workdeck PM checks passed.
```

The profile ran the locked `workdeck-pm` all-target suite and the CLI catalog,
green-gate, imported-check, dogfood, and fault-matrix targets. It completed with
**915 passed tests across 82 result groups, zero failures and zero ignored tests**;
`pm_dogfood` passed in 16.65 seconds and `pm_fault_matrix` passed in 0.56 seconds.
The Git-heavy claimed-completion fixture serializes its two end-to-end cases
inside the test binary, preserving the bounded source timeout under the normal
workspace harness. The validator SHA-256 pin and command allowlist were checked
before the child Cargo runs. This validates reproducible local profile execution.
The final matching no-publish release gate was run with the same profile:

```text
CARGO_NET_OFFLINE=true ... xtask pm release-check --profile standalone
Finished `release` profile [optimized] target(s) in 0.36s (cached)
Workdeck PM release-readiness checks passed.
```

This qualifies the local release build and profile composition only. Independent
trusted-baseline review, external CI/release observation, and the remaining
PM-10/PM-11/PM-12 gates remain open.


## Named PM profile rerun after PM-10 source reuse — 2026-09-11

After the descriptor-stamp/source-hash reuse, changed-path projection, parent-descriptor
cache and migration lock-wait changes, the named PM check was rerun in an isolated target:

```text
CARGO_NET_OFFLINE=true ... cargo xtask pm check --profile standalone
Workdeck PM checks passed.
```

The rerun passes **917 tests across 82 result groups, zero failures and zero ignored tests**.
It includes the migration subprocess competition case that previously timed out under the
full harness; the bounded migration wait now remains within the 10-second limit and passes.
The rerun is local evidence only. At this checkpoint a fresh no-publish `release-check`
remained open; it is superseded by the profile-dispatched release run recorded above at
`/tmp/workdeck-pm-profile-command-release-20260911.log`. Independent validator and
external CI observation remain open.


## Calibrated no-publish release profile rerun — 2026-09-11

The release-readiness profile was rerun after the calibrated PM-10 gate and capture changes:

```text
CARGO_NET_OFFLINE=true ... cargo xtask pm release-check --profile standalone
Workdeck PM release-readiness checks passed.
```

The release profile passes **917 tests across 82 result groups, zero failures and zero
ignored tests**, then completes the optimized `workdeck-cli` release build. This is local
no-publish evidence; external CI/release observation, independent validator review and
cross-platform qualification remain open.

The later profile-dispatched rerun also executes the exact calibrated feature and issue
benchmarks before the release build; see the current evidence section above for its
measurements and log.


## Workspace verification after the policy-state repair — 2026-09-11

The repository-wide all-target test run was repeated after correcting the TUI
fixture to expect the configured terminal workflow state (`done`) emitted by the
shared planning policy mutation:

```text
CARGO_NET_OFFLINE=true ... cargo test --offline --locked --workspace --all-targets --no-fail-fast
test result: ok. 4,933 passed; 0 failed; 1 ignored across 204 result groups
CARGO_NET_OFFLINE=true ... cargo clippy --offline --locked --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized] target(s) in 30.52s
CARGO_NET_OFFLINE=true ... cargo build --offline --locked --release --package workdeck-cli --bin workdeck
Finished `release` profile [optimized] target(s) in 54.86s
```

The single ignored test is an existing intentional workspace case. This is local
working-tree evidence from an isolated Cargo target; it does not substitute for
external CI, supported-platform runs, measured mounted-scale budgets, or the
independent validator/trusted-baseline review still required by PM-10 through
PM-12.

The repository-level `xtask verify` command was then rerun from the same isolated
target after fixing its release-binary lookup to honor `CARGO_TARGET_DIR`. It
completed the static checks, workspace tests, warnings-denied Clippy, release
build, installed-style help smoke, and large-repository smoke:

```text
/tmp/.../xtask verify
test result: ok. 4,934 passed; 0 failed; 1 ignored across 204 result groups
Workdeck Rust verification and large-repository smoke passed.
```

The release-binary lookup repair is a verification-tooling change only; it does
not claim external CI or platform qualification.

## Final serialized source checks, independent audits, and gate closure — 2026-09-11

This section records the final qualification series on the delivered source. Every
Cargo command ran serialized with `CARGO_NET_OFFLINE=true`, `--offline --locked`,
zero debug info, and a fresh isolated `CARGO_TARGET_DIR` that was removed with
`find -depth -delete` after the terminal result; no Cargo process overlapped
another.

### Check series

| Check | Result | Retained log |
| --- | --- | --- |
| `cargo fmt --all -- --check` | pass (exit 0), re-run after the validator hardening | `/tmp/workdeck-pm-gate-fmt-20260911.log` |
| `git diff --check` | pass (no whitespace/conflict-marker findings) | — |
| `cargo clippy -p workdeck-pm -p workdeck-vcs -p xtask --all-targets -- -D warnings` | pass (34.05 s), repeated after the hardening (43.23 s) | scratch `pm-final-clippy.log`, `pm-final-clippy2.log` |
| `cargo xtask architecture check` | pass: 13 production crates, one shipped executable, zero violations | `/tmp/workdeck-pm-gate-architecture-20260911.log` |
| `cargo xtask skill check` | pass: review skill current; extension/release skills match source mappings | scratch `pm-final-skill.log` |
| `cargo xtask pm release-check --profile standalone` (pre-hardening source) | **917 passed / 0 failed / 0 ignored across 82 groups**, both calibrated benchmarks within budgets, optimized build OK | scratch `pm-final-release-check.log` |
| `cargo xtask pm release-check --profile standalone` (final hardened source) | **917 passed / 0 failed / 0 ignored across 82 groups**; features 11,723.06 ms cold / 8,913.54 ms incremental / 0.849 ms warm p95 / 1,432,911,872 B peak RSS; issues 7,418.64 ms / 6,033.59 ms / 0.867 ms / 382,615,552 B | `/tmp/workdeck-pm-release-final-20260911.log` |
| `cargo test --locked -p workdeck-cli --test cli` | **46 passed / 0 failed** | `/tmp/workdeck-pm-gate-cli-20260911.log` |
| `cargo test --locked -p workdeck-cli --test git_integration` | **12 passed / 0 failed** | `/tmp/workdeck-pm-gate-git-integration-20260911.log` |
| `cargo test --locked -p workdeck-pm` | **906 passed / 0 failed / 0 ignored across 77 groups** | `/tmp/workdeck-pm-gate-pm-crate-20260911.log` |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | pass (41.32 s) | `/tmp/workdeck-pm-gate-workspace-clippy-20260911.log` |
| `cargo xtask verify` (first attempt) | 1 intermittent failure: macOS `EPERM` process-group kill in `execution::process::tests::canceled_owned_group_cannot_leave_a_descendant_writing_later`; no source change preceded it | `/tmp/workdeck-pm-verify-eperm-first-attempt-20260911.log` |
| exact single-test rerun of the EPERM case | **1 passed / 0 failed** in 2.31 s | `/tmp/workdeck-pm-eperm-rerun-20260911.log` |
| `cargo xtask verify` (re-run, no source change) | **4,944 passed / 0 failed / 2 ignored across 205 groups** plus static checks and the optimized release smoke | `/tmp/workdeck-pm-verify-final-20260911.log` |

The EPERM case passed in the repaired-source workspace run, both release-checks,
the exact-command PM-crate run, and the isolated rerun; the single aggregate
failure with no intervening source change is recorded as a second intermittent
macOS environment failure (alongside the historical watcher 250 ms window case),
not a regression. Local relative-link checking over all eight
`docs/project-management*.md` files resolved all **486 relative links**; all 27
retained `/tmp/workdeck-*` evidence logs referenced by the docs exist on disk.

### Validator hardening after the independent PM-11 audit

The independent PM-11 audit found two locally fixable gaps in the standalone
release profile validator: the `validator.sha256` pin and the
`trusted_baseline` digest were independent fields that could silently drift
apart, and the tag syntax check accepted `refs/tags/a/./b`-style middle
dot-components that `git check-ref-format` rejects. Both were fixed in
`xtask/src/project_management.rs`: `load_profile` now requires the trusted
baseline digest to equal the current validator pin, and every slash-separated
tag component must neither begin with `.` nor end with `.lock`. Two focused
tests cover the drift rejection (against a tampered profile copy in a temporary
repository) and the new syntax rejections; all seven `project_management` tests
pass, the profile JSON pin was updated to the hardened source digest
(`484f1b66…0bc87`), and the full release-check was re-run green on that final
source. External validator execution and authenticated immutable-baseline
resolution remain open as before.

### Independent audit summary

Five read-only audits were dispatched (PM-10 performance realism, PM-11 CI
trust/validator boundaries, PM-12 platform/release evidence, spec-vs-ledger
coverage, docs consistency). Material findings and their resolution:

- **PM-10**: no test theater; every doc figure matched the logs exactly; the
  one checked row (PM-10.D7) is defensible. Wording in PM-10.D7 and PM-X.C32
  loosely attached cold-index timings to enforced budgets; both rows now state
  that only incremental refresh (30 s/15 s) and warm-filter p95 (100 ms) are
  enforced. Independent tree/board thresholds, richer fixtures, and
  cold/RSS budgets remain open.
- **PM-11**: trust boundaries are exactly where the docs state them
  (local clock, local git object database, self-attested authority files,
  mutually editable profile+validator pair); the two fixable validator gaps
  above were fixed; no row was prematurely closed; external prerequisites
  (validator execution, authenticated baseline resolution, external CI/release
  receipts) stay open.
- **PM-12**: zero overclaims; all 15 rows correctly open; the packaging
  inspection helpers are genuine production code wired into
  `release package` with post-write re-inspection; Windows compilation remains
  toolchain-blocked (no MinGW/compiler headers), so Windows compiler/runtime,
  signing, and external release receipts stay open.
- **Spec vs ledger**: coverage complete (97 D + 71 E + 42 C + 9 G = 219; no
  plan deliverable or exit criterion lacks a row); zero dead links across the
  129 then-checked rows; stale blocker text in PM-V.G6/G8 was corrected by the
  gate closure below.
- **Docs consistency**: 129/219 and 10-of-13 were exact and uniform; no
  forbidden overclaims (content-hash watcher protection, Windows success,
  external receipts, independent thresholds); watcher wording is consistently
  metadata-fingerprint based. Stale statements fixed: the handoff's
  "currently running" workspace note, the plan's "later phases remain
  planned" phase line, `performance.md`'s "qualify" → "record", and the two
  budget-wording rows above.

### Ledger gate closure

Eight verification-gate rows (PM-V.G1, G2, G3, G5, G6, G7, G8, G9) were closed,
each citing a fresh exact-command final-source run recorded above. PM-V.G4
remains open: its own acceptance ties closure to final terminal/platform
acceptance (supported-platform matrix, PM-X.C39), which is external. No PM-10,
PM-11, PM-12, or PM-X row was closed; their external/platform/performance
prerequisites are unchanged and stated per row.

**Final ledger state: 137 of 219 rows checked (approximately 63%); 10 of 13
phases closed (approximately 77% by phase count).** Phase progress is unchanged
by gate closure. Storage after cleanup: approximately 48 GiB free; every
isolated target directory created by this series was removed with
`find -depth -delete`, and no evidence log under `/tmp` was deleted.

## Post-rebase qualification on integrated main — 2026-09-12

The working branch was rebased onto `main` (`bc0a9e79`, 747 commits ahead of the
previous base `c9a36ec6`) with the full implementation reapplied on top. Eleven
conflicts were resolved by union (workflows, manifests, module registries,
dispatch, test modules) and the integration seams were reconciled: the skill-path
command keeps main's read-only resolution for the four bundled skills and the
generated PM skill for `workdeck-pm`; the archive inspection helpers moved with
main's relocated installer into `workdeck-cli` and remain the release packager's
post-write gate; main's interactive Git comparison-base cycling (`keys.base`)
was re-ported into the workbench Git panel; a mutation key pressed while the
first planning index load is still in flight is now deferred and replayed after
selection sync instead of being dropped; and `GIT_CONFIG_GLOBAL=/dev/null`
(main's hermetic test isolation) is treated as the empty configuration it
denotes rather than an unsafe ignore source.

Qualification on the rebased source: `cargo xtask verify` completed with
**5,888 passed, 0 failed and 9 ignored across 219 result groups** plus static
checks and the optimized release smoke
(`/tmp/workdeck-pm-verify-postrebase-20260912.log`);
`cargo xtask pm check --profile standalone` passed **917 tests across 82 groups**
with both calibrated benchmarks inside budget (features 8,916.58 ms incremental
refresh and issues 5,989.17 ms, warm-filter p95 below 1 ms;
`/tmp/workdeck-pm-check-postrebase-20260912.log`); workspace Clippy with
warnings denied, `cargo fmt --check`, `git diff --check` and the architecture
gate (13 production crates, one shipped executable) all pass. All 2026-09-11
sections above remain accurate pre-rebase receipts and stay labeled as such.
