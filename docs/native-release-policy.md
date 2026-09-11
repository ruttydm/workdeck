# Native release-channel and version policy

## Current semantic-port closeout (`ee34bf1e`)

The latest local closeout reran the strict port gates after the source-presentation-keyed
geometry cache change. `cargo xtask port fetch` followed by strict `cargo xtask port audit`
report 1,257 baseline files, 1,459 records, zero unmapped records, zero post-baseline upstream
commits, and 1,197 provenance-checked Workdeck commits. The native 20-sample interaction
receipt is recorded at
[`port/hunk/benchmarks/interaction-native-ee34bf1e.json`](../port/hunk/benchmarks/interaction-native-ee34bf1e.json);
against the frozen same-host Hunk pins it passes every paired interaction metric (scroll tick
median 0.60 ms versus 1.93/2.04 ms, 68.9%/70.6% under the main/stable pins), and three fresh
`/usr/bin/time -l` native rounds peak at 119,013,376 bytes against frozen pin peaks of
613,482,496 and 591,872,000 bytes, so the paired 10% latency gate and the no-peak-memory
regression requirement pass on this host. Non-macOS execution, remote signing/provenance, and
external installer/update evidence remain release gates rather than being inferred from the
local pass; the recorded blockers live in the goal evidence notes.

The prior closeout's complete Rust verifier run, formatting, dependency policy,
workspace/all-target tests, Clippy, release build, large-repository smoke, and local artifact
path (`release build`, host staging, `release check-artifacts`, `install-inspect --package`,
`install-verify`) all passed and are re-run by the recorded gate suite; the archive carries a
locally bound unsigned statement (`checksumVerified: true`, `signatureVerified: false`), and
`release package --verify-ci` correctly refused that input because a real Sigstore bundle was
absent. This validates packaging mechanics, not publisher authenticity or release readiness.

## Dependency-policy checkpoint (`bf8b7cf7`)

On the macOS development host, `cargo deny check` exits successfully with
`advisories ok, bans ok, licenses ok, sources ok`. The checked policy lists all
five native release targets and enables all features. It retains three explicit
advisory exceptions (`RUSTSEC-2025-0141`, `RUSTSEC-2024-0320`,
`RUSTSEC-2024-0436`), so this result is policy compliance, not a claim of an
exception-free advisory scan. Duplicate crate versions remain warnings. The
unused `MPL-2.0` and `Unicode-DFS-2016` allowances also produce warnings; the
policy was not weakened or changed to obtain the passing result.

Release packaging obtains its license inventory from locked, offline Cargo
metadata and serializes package name, version, license, repository and source
in deterministic order. This mechanism was inspected at this checkpoint; no
signed archive was generated or verified by the dependency-policy command.
Native installation, full third-party artifact contents, signature/provenance
verification and semantic-port parity remain separate release gates.

## Release input policy

These Rust tools validate release inputs and emit metadata. They do not fetch a latest version,
create a Git tag, commit changes, upload assets, or publish packages or releases.

`cargo xtask install-plan [version] [--no-modify-path] [-f|--force]` is a read-only installer
preflight. It reports version/flag selection and native OS/architecture as JSON, including Rosetta
correction on macOS. `WORKDECK_VERSION`, `WORKDECK_NO_MODIFY_PATH` and
`WORKDECK_ALLOW_CONFLICTING_INSTALLS` supply defaults; boolean environment values enable flags
only when exactly `1`. Last positional version wins, and one leading `v` is removed. The output
explicitly sets `executionAvailable: false`: release resolution, conflict checks, verified archive
extraction, atomic installation and shell-profile updates remain unfinished. This is not an
installer or updater replacement yet and must not justify removing the existing implementation.
Preflight also reports the target binary (default `~/.workdeck/bin/workdeck`, or `workdeck.exe`
on Windows; overridden by `WORKDECK_INSTALL_DIR`) and existing files found at PATH entries.
Parent directories and up to eight executable symlink hops determine identity; diagnostics
classify whether each observed path precedes or follows the target. Observations are grouped by
canonical identity in first-PATH-occurrence order while retaining distinct aliases for future
manager-aware diagnostics; aliases of the target itself are excluded. The first recognized alias
supplies `diagnosticPath` and `managerHint` for Cargo, Homebrew, Nix, mise, or standalone layouts.
These are layout-based hints, not verified ownership; `/usr/local/bin/workdeck` remains ambiguous.
Empty PATH entries resolve
against the current directory without changing the process working directory. Candidate programs are never
executed. `executableAccess` uses the OS effective-user execute-access check on Unix and is
`null` on Windows, where native access validation is unfinished; executable format is not
validated by launching the candidate. File observations are not full conflict validation:
Windows permission checks, manager-aware remediation and legacy-manager scans are still pending. `existingInstallFiles`
combines PATH observations with the bounded `~/.local/share/mise/installs/workdeck/*/workdeck`
and `*/bin/workdeck` layouts (using `workdeck.exe` on Windows), preserving identity deduplication.
Off-PATH installations are labeled `not-on-path`. Missing mise directories create no state;
other directory-read failures are reported rather than silently hiding candidates.
`observedConflictDecision` reports `requires-force` for observed executable competitors unless
`--force` explicitly allows them. Non-executable files do not count as executable competitors.
Unknown access yields `unresolved-access`, including when force is set; it is not a successful
permission check. `no-observed-executable-conflicts` describes only this incomplete discovery
scope. Every preflight still reports `executionAvailable: false` and performs no installation.
Unix access-probe failures other than confirmed denial/absence remain unknown. Metadata failures
are retained as unresolved observations, not omitted; a real symlink-loop regression verifies
that such a candidate cannot become a clean conflict decision even with force enabled.

`cargo xtask install-verify ARCHIVE CHECKSUM_FILE` verifies a local archive against an exact,
unique SHA-256 entry in either a release-wide checksum manifest or a per-archive checksum file.
Missing, malformed, duplicate and mismatched entries fail. Hashing reuses the native packaging
implementation; no external checksum executable is required. Unlike Hunk's historical fallback
for releases lacking checksums, this path never accepts an unverified archive, consistent with
the strict Workdeck release gate. Successful JSON states `checksumVerified: true`,
`signatureVerified: false`, and `installed: false`: this step neither authenticates the checksum
manifest nor validates/extracts the archive or installs anything. Signature/provenance validation,
safe extraction and atomic replacement remain required before installation execution is enabled.

`cargo xtask install-inspect ARCHIVE` reads tar.gz or ZIP entries without extracting files.
It rejects absolute/traversal paths, backslashes, drive/stream separators, reserved DOS names,
trailing dots/spaces, links/special files and case-folded duplicate paths. Checks
also reject files used as parent directories in either entry order and directories carrying
payload bytes, while permitting explicit empty directory entries. Inspection limits
archives to 100,000 entries and 2 GiB of declared uncompressed payload. Each payload read is
bounded to its declared size plus one byte and must
match the declared size exactly; short and oversized streams fail without unbounded draining.
This is preliminary
structural inspection, not a complete package verifier: required contents, wrapper layout,
signatures/provenance and safe extraction remain unfinished. Output deliberately does not claim
checksum verification or installation. Tests exercise tar and ZIP payload reads and path rejection
without creating extracted directories.
Tar inspection also drains the gzip decoder after tar's end marker to validate CRC/ISIZE,
permits at most 1 MiB of trailing zero tar padding, and rejects nonzero padding, extra compressed
members and trailing compressed-stream bytes. Regression tests cover corrupt/truncated trailers,
hidden trailing payloads, concatenated members and excessive zero padding without extraction.
Checksum manifests are read from validated regular-file handles with a 1 MiB limit and UTF-8
validation. The reader stops after at most one excess byte; tests cover the exact boundary,
oversized and invalid-UTF-8 inputs, and directory rejection. This bounds manifest input only;
checksum matching still does not establish publisher authenticity.
Archive checksum hashing uses the validated handle and reads at most the observed length plus
one byte, capped by the 2 GiB input policy. Shrinking or growing inputs fail length validation.
Tests exercise known SHA-256 vectors, short/growing streams and bounded reads. Same-length
concurrent content changes are not detected by length alone: the resulting digest must still
match the supplied checksum, and immutable authenticated extraction remains a separate gate.
Before parsing, archive inspection requires a regular file of at most 2 GiB compressed bytes,
and repeats the metadata check on the opened handle. Unix tests use sparse files to verify the
exact size boundary without allocating GiBs. This is not a complete allocation bound: ZIP central
directory allocation still occurs inside the dependency before entry iteration and needs its own
limit. Unix archive opens use `O_NONBLOCK` and validate the opened handle, so a FIFO substituted
after the initial path check is rejected without waiting for a writer. A native FIFO regression
exercises that open path directly. This does not prevent concurrent modification of a regular file;
immutable verified extraction inputs and equivalent Windows handling remain unfinished.

`install-inspect ARCHIVE --package` additionally requires one wrapper directory, exactly one
`workdeck` or `workdeck.exe` regular file, and regular `LICENSE`, `THIRD_PARTY_NOTICES`,
`licenses.json`, `sbom.cdx.json` and `provenance.json` files with their exact spelling. These are
path/type checks only, not validation of metadata contents or provenance authenticity.
The packager now requires `--provenance STATEMENT_OR_BUNDLE`; missing input fails rather than
generating placeholder evidence. It retains the statement byte-for-byte as `provenance.json`.
For supported Sigstore 0.2/0.3 DSSE bundles it also retains the entire original input byte-for-byte
as `provenance.sigstore.json`. Bundle decoding enforces the media/payload type, one nonempty
base64 signature, and the input size limit, but does not authenticate those bytes or validate
certificate/transparency-log material. Synthetic test signatures are deliberately not trusted.
Bundle format reference: [Sigstore bundle specification](https://github.com/sigstore/protobuf-specs/blob/main/protos/sigstore_bundle.proto).

`cargo xtask release provenance-check BINARY STATEMENT` provides a read-only input-boundary
check for a plain in-toto Statement v1 with the SLSA provenance v1 predicate. It bounds statement
reads to 1 MiB, requires nonempty build-type and builder identifiers, and requires exactly one
subject matching the binary basename with its actual SHA-256. Duplicate matching subjects,
wrong envelopes and mismatched digests fail. Unknown fields remain accepted. This is not full
schema validation, DSSE/Sigstore verification, builder trust, or verification of build inputs;
successful output explicitly reports `signatureVerified`, `builderTrusted` and `releaseReady`
as false. It neither creates provenance nor packages or installs anything. The synthetic unit
test statements are test data, not build evidence.
Format reference: [SLSA provenance v1](https://slsa.dev/spec/v1.0/provenance).

`cargo xtask release package --target TRIPLE --provenance STATEMENT [--binary PATH] [--output DIR]`
checks that statement against the exact in-memory executable bytes passed to the archive writer,
not an earlier hash of a subsequently reopened binary. Invalid evidence fails before creating the
output directory. Both tar and ZIP tests reopen archives and verify exact binary and statement
bytes. This remains subject binding, not authentication: signature verification and trusted builder/input policy
are unfinished. The release workflow now requests binary attestation before packaging, supplies
the action's bundle output, then requests separate archive attestation. This workflow wiring has
not been executed remotely; successful local decoding tests are not signing/CI evidence.
Nothing has been published or represented as a verified release.

`cargo xtask release provenance-verify BINARY BUNDLE OWNER/REPO SOURCE_COMMIT refs/tags/TAG`
invokes the declared native `gh` binary's attestation verifier without a shell. It requires the
explicit repository, `.github/workflows/release.yml` signer, complete source commit and tag ref,
GitHub Actions OIDC issuer, SLSA v1 predicate, and non-self-hosted runner. Nonzero exits, missing
tools and a 120-second deadline fail the command; timed-out children are killed and reaped.
The standalone command supports `--ci BINARY BUNDLE`, reading repository, commit and ref
directly from GitHub's environment rather than interpolating tag names into shell commands.
This relies on the installed verifier and its trusted roots;
it is not a home-grown cryptographic implementation. Local policy tests check argument enforcement,
not signatures. Real signed-fixture, negative-certificate, cross-platform and remote CI evidence
remain pending. CI now uses `release package ... --verify-ci`: it verifies private temporary
copies of the exact binary and bundle bytes held in the archive entries before creating output.
The original paths are not reopened after verification. Snapshot tests verify contents, cleanup
and Unix directory permissions; Windows ACL behavior still needs native verification.
Packaging without `--verify-ci` still performs only subject binding, and installer-side
verification of packaged evidence remains unfinished. The host is explicitly `github.com`,
independent of a user's `GH_HOST`. Native subprocess tests exercise successful exit, nonzero exit,
and deadline termination/reaping using the Rust test executable; these are process-control tests,
not synthetic substitutes for cryptographic verification evidence.
Verifier reference: [GitHub CLI attestation verification](https://cli.github.com/manual/gh_attestation_verify).

Verification checkpoint at `57769da3`: `cargo test -p xtask -- --quiet` passed all 161 tooling
tests together, including the installer checks and production benchmark tests. This is a local
development-profile integration result, not evidence of cross-platform installation, signed
provenance, release-gate completion, or acceptable optimized performance. At that checkpoint,
packaging had no provenance input; the input boundary above was added subsequently. Authentication
and validation of actual build evidence remain outstanding.

Integration checkpoint at `c91fdef7`: all 169 `xtask` tests passed together (40.39 seconds,
local development profile), including provenance input, archive retention and subprocess tests.
Strict `cargo xtask port audit` was rerun successfully after rebuilding the audit executable,
and returned failure as required: 1,257 baseline files, 1,326 records, 315 unmapped records,
five tracked stable-only commits and 11 commits in the locally tracked upstream delta.
No fresh remote fetch was performed for this checkpoint. These counts do not establish functional
parity or passing release gates. The first audit attempt failed at linking due to exhausted disk
space; only regenerable `xtask` development build output was cleaned before the retry.

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

`cargo xtask benchmark parse-metrics` reads benchmark stdout from stdin and emits ordered
`[name, value]` pairs. It preserves the first insertion position and last value of duplicate
names, uses the pinned decimal-number grammar and ECMAScript whitespace rules, and ignores
non-metric output. Five frozen cases match both pins. This parser is deliberately separate from
workload execution. Native report models also retain runtime, version, sample-count and regression
explanation metadata; a read-only test round-trips all 22 pinned historical release reports.
Historical runtime metadata is data only and never starts a JavaScript runtime.
The benchmark-result source mapping covers thresholds, metrics, runtime metadata, regression
explanations, run/comparison records, classification and nearest-rank aggregation. It does not
cover the separate runner, workload execution, or the final same-host performance gate.

`cargo xtask benchmark runner-plan` parses the runner's `--samples`, `--out`, repeatable `--script`,
`--include-huge` and `--include-competitors` options. Native defaults use
`WORKDECK_BENCHMARK_SAMPLES` and `WORKDECK_BENCH_INCLUDE_HUGE`. Nine dual-pin cases cover defaults,
fractional/radix sample counts, repeated values and errors. The plan preserves source workload
identifiers (including their historical `.ts` suffix), ordered defaults and appended opt-ins;
these identifiers do not name executable source files in Workdeck. The plan reports whether its
entire selection has native execution support, creates no output directory and executes no
workload. The default suite and the `--include-huge` plus `--include-competitors` selection now
report `executionAvailable: true`; the opt-in huge driver emits native RSS and, where available,
allocator-in-use metrics without relabeling them as a JavaScript heap.

`cargo xtask benchmark run --script render-layout.ts --samples 1 --out REPORT.json` executes
the completed render-layout workload in a native child process, drains both output pipes,
aggregates repeated samples and writes a versioned report with Git/Cargo/native-platform metadata.
`bootstrap-load.ts`, `working-tree-load.ts`, `changeset-parse.ts`, `highlight-prefetch.ts`, `large-stream.ts`,
`non-ascii-stream.ts`, `wrapped-cjk.ts`, `render-layout.ts`, and the opt-in `huge-stream.ts` are
admitted. Every selected workload is checked before execution or output-directory creation;
unknown or unavailable selections fail before any child starts.
Fractional sample counts retain the source loop semantics, and repeated workload selections append
samples rather than replacing them. Historical metric thresholds remain compatibility metadata.
Child stdout/stderr use replacement UTF-8 decoding with one initial BOM removed, matching the
pinned runtime's text decoding. Unix signal exits use `128 + signal` (SIGTERM is 143); normal
nonzero exits retain their numeric status, emit trimmed stderr and do not forward failed stdout.
Frozen runtime-primitive cases and an actual signal-terminated child test cover these paths.
The native runner now covers the pinned executable workload set, while optional profiling helpers
remain explicit commands rather than release-runner inputs. General locale-sensitive report
ordering and the strict same-host performance gate remain separate evidence requirements.

`cargo xtask benchmark render-layout` measures split rows, stack rows, section geometry and
review plans for the three pinned size/shape scenarios. Native row counts match both oracles,
including the 18,000-line single-file case. Stream content retains the source's declared statistics
and full before/after snapshots. No same-host latency or memory acceptance is inferred from these
count tests.

`cargo xtask benchmark stream-fixture [--huge | --non-ascii]` constructs native fixture bootstraps
and prints their summaries without creating repository state. The default is 180 files of 120
lines; huge mode combines 1,000 files of 300 lines with one directly synthesized 50,000-line file.
The giant patch avoids a large diff calculation and remains partial metadata, while ordinary
stream files retain complete sources. Eight custom-file cases verify source hashes, statistics
and hunk geometry against both pins; normal, non-ASCII and complete huge bootstrap summaries are
also checked. These fixture constructors are separate from the production-loader benchmark and
strict performance acceptance gate.

This workload exposed and fixed a product-level snapshot bug: `diff_from_file_snapshots` now marks
complete text comparisons non-partial and retains both full sources, so trailing collapsed gaps
remain visible. Patch-only parsing remains partial. Diff-library and live terminal tests cover
the change; the benchmark oracle detects the formerly missing row per balanced-stream file.

`cargo xtask benchmark working-tree` creates disposable Git fixtures for the five pinned tracked
and untracked scenarios and emits native `METRIC` lines. Structural file/addition/deletion counts
are checked against both pinned runtimes. Its timed boundary now includes the bundled catalog,
production `load_selected_vcs_changeset` path and shared `AppBootstrap::new` assembly. The CLI uses
those same implementations. Config/extension preparation remains outside this source-level
bootstrap benchmark, just as it is outside Hunk's measured `loadAppBootstrap` call.
Single oracle timings in `benchmark-working-tree.json` overlapped other validation and are explicitly
excluded from the same-host performance acceptance evidence.

`cargo xtask benchmark bootstrap-load` reproduces the 64-file/420-line fixture, direct-file pair,
Git subprocess, parsing and patch-chunk probes. Complete source hashes for ordinary and pair files
match both pins, as do all structural counts. The native parser probe uses the production metadata
stage without constructing review files, retaining the same file-count check. Explicit cwd arguments
preserve source/reload context without temporarily changing the process-global cwd. Fixtures disable
Git signing, preserve committed-before/current-after bytes and remove their owned directory on drop.
The shared constructor owns input-derived defaults; the CLI still attaches its configured themes,
extension state, notices, keybindings and preference destination afterward. This refactor does not
move agent/process ownership or bypass extension preparation in the actual product.

`cargo xtask benchmark changeset-parse` measures normalization, metadata parsing, chunk splitting
and review-file construction separately for 240 small files, 96 balanced files and one large file.
The production parser shares its metadata and construction stages with this workload; construction
still resolves language, counts additions/deletions, detects binary patches and assigns identity.
All patches are prepared before measurements. Both pinned oracles agree on file and sanitized-byte
counts, and native tests compare complete resulting files with the production changeset path.
Diagnostic oracle timings are not performance acceptance evidence.

`cargo xtask benchmark highlight-prefetch` uses the production Ratatui app and 240×24 cell buffer
for the four pinned marker files. It reconstructs contiguous styled spans from cells and applies
the source marker-segmentation check, preserving both bounded polling loops, the two intervening
frames, Down input, zero-on-timeout metrics and owned-runtime cleanup. Down is the source's
one-row review command, not a substituted next-file command despite the historical metric name.
Native tests verify complete fixture sources and geometry, selected/adjacent highlighting,
non-no-op selection input, and rejection of plain unsegmented marker text. Scheduling uses the
native renderer and thread yield; diagnostic timings and observed iteration counts are not fixed
scheduler contracts or performance acceptance evidence.

`cargo xtask benchmark large-stream` uses fresh 180-file/120-line production apps for cold and warm
first frames and four wheel ticks at (170,12), in a 240×28 viewport. Fixture/app setup is outside
each timer. Scroll timing retains the source's 17 ms coalescing pause per event; selected-marker
highlight settlement (up to 200 frames) and app retirement are outside the measured intervals.
There is no React test scheduler in this native rendering path. Tests require highlighted first
frames and actual viewport movement. Debug execution of the full workload is currently expensive;
functional coverage must not be interpreted as satisfying the optimized latency/memory gates.
The [recorded optimized investigation](../port/hunk/benchmarks/README.md) also fails the 10%
latency limit against both pins; it retains raw three-sample reports and reproduction commands.

`cargo xtask benchmark non-ascii-stream` measures a fresh 120-file/120-line Unicode stream's
first frame and a separate warmed app's eight individual wheel ticks at (170,12), using the same
240×28 production viewport. It emits first-frame latency, nearest-rank scroll median/p95 and
structural counts. Unlike the large-stream workload, the source interaction helper has no explicit
17 ms coalescing delay; native event dispatch/render/yield is timed per tick. CJK, emoji and
box-drawing content is retained in both full source snapshots. App destruction is outside the
timers. Frozen direct-workload output from both pins is diagnostic only, not controlled performance
acceptance evidence; final latency and memory gates remain outstanding.

`cargo xtask benchmark wrapped-cjk` reproduces the 518-line Japanese Markdown issue shape and
an 8,736-UTF-16-unit single line in a 240×60 wrapped split view. First-paint timing includes
production app creation; wheel timing excludes explicit synchronous highlighting into the app's
own cache and initial viewport settlement. Twelve wheel events are dispatched as a burst before
rendering, followed by a second frame after the source's coalescing delay. Character-only frame
comparisons reject no-op scrolling; Japanese-content row floors expose blank first/burst frames.
Dual-pin observations and native tests agree on 54 initial and 55 immediate/settled content rows.
These geometry checks are not latency or memory acceptance evidence.

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
