# Native release-channel and version policy

These Rust tools validate release inputs and emit metadata. They do not fetch a latest version,
create a Git tag, commit changes, upload assets, or publish packages or releases.

```console
cargo xtask release channel --event push --ref v0.19.0 --current-latest 0.18.2
cargo xtask release channel --event workflow_dispatch --ref main --requested-tag beta
cargo xtask release check-version v0.1.0
```

The channel command prints one JSON object with `channel` and `makeLatest`. A push of a newer
stable version selects `latest` and permits latest-release promotion. An older stable version
selects `backport-MAJOR.MINOR` without promotion. Republishing the current latest version fails.
An alpha, beta, or release-candidate reference selects `beta` without promotion. A manual dispatch
requires an explicit nonblank channel; only the exact trimmed value `latest` permits promotion.
The caller supplies the current latest version from its verified release source; these commands
do not query an npm registry or assume authority to publish.

Stable parsing retains the pinned source's optional leading `v`, three numeric components,
leading-zero acceptance, and numeric comparison behavior. Prerelease matching retains its source
pattern rather than imposing an additional semantic-version validator. Argument parsing accepts
pairs, retains unknown pairs, and uses the last duplicate value, matching the source script.
Missing pairs and unsupported events fail. Native metadata calls the field `channel` instead of
the source's npm-specific field. Workdeck has no npm publication workflow.

The version command reads the sole executable package, `workdeck-cli`, through Cargo metadata
and requires the exact `v<package-version>` tag, including a prerelease suffix when present. It
prints a confirmation on success and fails on a missing, unprefixed, or mismatched tag. The sample
version above must be replaced with the actual package version when preparing a later release.

Evidence is in [the dual-pin release-channel oracle](../port/hunk/oracles/release-channel.json)
and the Rust tests in `xtask/src/release_channel.rs`: all seven source tests are translated, with
18 frozen differential cases plus argument and Cargo-version checks. Both pinned source suites
passed seven tests and ten assertions. Attribution is retained in
[THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES).

These helpers do not complete native release preparation, installation tests, artifact signing,
cross-platform CI, or the remaining release gates. See [the semantic-port ledger](../port/hunk/README.md).

## Benchmark aggregation boundary

`cargo xtask benchmark working-tree` creates disposable Git fixtures for the five pinned tracked
and untracked scenarios and emits native `METRIC` lines. Structural file/addition/deletion counts
are checked against both pinned runtimes. The current timed boundary is bundled VCS catalog
loading plus changeset materialization, not complete application bootstrap. That workload source
file remains unmapped.
Single oracle timings in `benchmark-working-tree.json` overlapped other validation and are explicitly
excluded from the same-host performance acceptance evidence.

The shared fixture generator is native in `xtask/src/benchmark/fixtures.rs`: it supports configurable
line counts/change regions, patch prefixes/extensions, committed-before/modified-after repositories,
untracked sources, Git diagnostics and automatic temporary-directory cleanup. Source and patch
bytes match twelve dual-pin fixtures; repository tests verify commit identity, custom extensions,
untracked paths and cleanup. Generated TypeScript-like content exists only as benchmark input,
not executable tooling or a source mirror. Fixture commits disable user signing agents.
`cargo xtask benchmark synthetic-patch OPTIONS_JSON` emits the deterministic patch without writing
repository state. Options retain camel-case names such as `fileCount`, `changedStart`, and
`changedLines`; fractional counts are truncated and negative counts generate empty arrays, while
changed-region bounds retain their numeric comparisons.

`cargo xtask benchmark aggregate SOURCE METRIC SAMPLES_JSON` emits native JSON with the
source-compatible nearest-rank median, p75, p95, extrema, original sample order, units, and metric
classification. Frozen aggregation outputs from both pinned runtimes are checked field-by-field
in `xtask/src/benchmark.rs`. The recorded historical thresholds (15% timing with a 5 ms floor;
20% memory with an 8 MiB floor) are compatibility metadata, **not** the semantic-port release
gate. They do not relax its 10% latency limit or zero peak-memory regression requirement.
`cargo xtask benchmark compare-json BASE_JSON HEAD_JSON` compares explicit version-1 snapshots
without writing files. It preserves historical missing-metric, informational, accepted-regression,
and failure statuses; failures return a nonzero exit. Ten dual-pin oracle cases check every result
field, including duplicate metric resolution and zero baselines. JSON parsing uses exact float
round-tripping so oracle precision is not silently rounded away. Historical acceptance annotations
are reporting compatibility only and cannot satisfy the stricter final performance gate.

`cargo xtask benchmark compare-markdown BASE_JSON HEAD_JSON` renders the historical report,
including failure counts, metric statuses, percentage deltas and absolute threshold units.
The full report and 24 decimal-rounding cases are frozen from both pins; exact binary-value
rounding avoids changing displayed values through intermediate floating-point multiplication.

`cargo xtask benchmark previous VERSION RELEASE_DIRECTORY` finds the latest stable snapshot
lower than the requested release, ignoring prerelease and unrelated filenames. A missing
directory or absent lower snapshot produces JSON `null`; invalid versions fail before lookup.

`cargo xtask benchmark release-plan [--version VERSION] [--samples N] [--out PATH]` resolves
native Cargo defaults and emits the run options without creating directories or executing a
benchmark. `WORKDECK_RELEASE_BENCHMARK_SAMPLES` supplies the default count (otherwise five).
An explicit output survives later version options; relative output paths resolve against the
current directory. Fractional positive sample counts retain the source parser's behavior.
This planning command is not a replacement for the still-unfinished suite execution boundary.

`cargo xtask benchmark compare-release` uses the executable's Cargo version and
`benchmarks/release/bench-VERSION.json`, selecting the previous lower stable snapshot unless
`--base` is supplied. `--head`, `--version`, and `--release-dir` override those inputs. Optional
`--out` writes comparison JSON (creating its parent directory); `--summary` appends the Markdown
report to an existing or new file without creating its parent directory. Failed comparisons still
write the requested evidence and report before returning failure. These are historical report
semantics, not authorization to accept a regression at the final semantic-port gate.

Benchmark execution, same-host measurements, complete source model and exported-helper parity,
and strict release enforcement remain incomplete. These tests are not performance measurements,
and the runtime source records remain unmapped. The complete nine-case comparison test file has
a translated-test mapping; this does not mark its runtime implementation or performance gate complete.

## Generated state and PR routing

`cargo xtask release validate-prerelease` reads the current executable's Cargo version,
`release/prerelease.json`, `CHANGELOG.md`, and regular Markdown files in `release/fragments/`.
It does not create those files. Prerelease JSON retains `mode`, `tag`, `initialVersions`, and
`changesets`; the initial version is keyed by `workdeck-cli`. Validation requires pre mode, a
nonempty channel tag, the highest stable changelog version as the initial version, agreement
between the current package version and channel, unique valid consumed fragment IDs still
present on disk, and the exact current-version changelog heading.

`cargo xtask release verify-pr-notes BASE_REVISION [HEAD_REVISION]` defaults the head to `HEAD`
and rejects option-like revisions. It classifies the NUL-delimited, no-renames Git diff. Only a
diff changing prerelease state and consisting entirely of release metadata uses generated-state
validation: prerelease JSON, changelog, root fragment Markdown, benchmark release JSON, or native
Cargo version metadata (`Cargo.toml`, `Cargo.lock`, and `crates/workdeck-cli/Cargo.toml`).
Ordinary changes, mixed source changes, and stable promotions removing prerelease state use the
ordinary gate below with the exact supplied base revision. Invalid generated state fails without
falling back to that gate. Neither route writes release state or publishes artifacts.

All 15 source verifier cases have native tests, including real Git routing fixtures. Historical
migrated fragments under `port/hunk/` are not active Workdeck fragments or prerelease state.

## Ordinary release-fragment status

`cargo xtask release status --since=REVISION` compares the current tracked working tree with the
merge base of the supplied revision and `HEAD`. It reads `release/fragments/*.md`, selecting
changed fragments that still exist. Modified existing fragments count; deleted fragments,
untracked files, hidden fragments, and case-insensitive README files do not. If tracked product
files changed without a qualifying fragment, the command fails. An unchanged tree needs no new
fragment, but the release-fragment directory must exist.

Fragments retain YAML frontmatter delimited by `---`, mapping Cargo package names to `major`,
`minor`, `patch`, or `none`. Empty frontmatter is an explicit maintenance-only fragment. Anchors
and merges are supported; malformed YAML, duplicate keys, invalid version types, and unknown
workspace package names fail. All workspace changes belong to the single Workdeck product;
internal crates are not independently shipped or version-bumped by this gate. JSON output records
the supplied revision, merge base, selected fragment IDs, and highest executable release type.
The command does not consume fragments, change versions, or publish anything.

Merge-base and surviving-fragment selection follow the pinned
[Changesets 2.31.0 Git helpers](https://github.com/changesets/changesets/blob/%40changesets%2Fcli%402.31.0/packages/git/src/index.ts)
and [status gate](https://github.com/changesets/changesets/blob/%40changesets%2Fcli%402.31.0/packages/cli/src/commands/status/index.ts).
The Rust parser uses `serde_norway`; no JavaScript runtime is used. Attribution is in
`THIRD_PARTY_NOTICES`, and `cargo xtask licenses` regenerates the dependency license inventory.
