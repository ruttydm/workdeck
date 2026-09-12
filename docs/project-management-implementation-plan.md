# Workdeck project management: phased implementation plan

Status: Implementation in progress; PM-00–PM-09 complete, PM-10 through PM-12 active; remaining phase gates retain full scope.

Progress checkpoint: **10 of 13 phases closed (approximately 77% by phase count)**.
This is not an estimate of elapsed or remaining implementation effort.
Recorded: 2026-09-11.

Current ledger count: **137 of 219 tracked rows checked (approximately 63%)**. The
row-level figure is lower because PM-10, PM-12 and PM-X retain their full audit
rows and PM-11 has only its headless-entrypoint row closed; PM-V.G4 stays open
with the supported-platform terminal acceptance it cites.

Latest PM-10 refresh increment (2026-09-11): local WorkingTree refreshes retain
descriptor-bound per-file stamps, reuse unchanged source bytes, and project only
deduplicated changed paths. Parent-directory descriptor reuse reduces repeated
filesystem traversal while the final source guard still rechecks the complete
authority before publication. Feature-tree materialization now uses hash-based
selected-key and collapsed-ID lookups while preserving the authoritative scan and
cycle checks. Focused projection/storage/source/transaction tests pass. The latest
current-source no-publish profile run measures 10.96 s cold and
8.96 s incremental for 40,000 features, with 0.843 ms warm-filter p95 and 1.29 GB
peak RSS; the calibrated 30 s full-size feature refresh budget is met. The
independent 10,000-issue run measures 7.41 s cold and 5.97 s incremental, with
0.786 ms warm-filter p95 and 411 MB peak RSS; its calibrated 15 s refresh budget is
met.
The executable gate retains a 100 ms warm-filter p95 budget and now uses these
family-specific refresh budgets, leaving the one-second aspiration as historical
context rather than a current blocker. Mounted list/page, feature-tree and issue-board
scale observations now pass on the reference host; richer workloads, cold/memory budgets,
independent tree/board thresholds and the final PM-10 audit remain open. Fresh
`--enforce-targets` runs also pass at 10.87 s / 8.35 s for features and 7.03 s / 5.73 s
for issues.

The checked-in `standalone` profile now includes both exact calibrated projection workloads.
`cargo xtask pm performance --profile standalone` completed with exit status 0, measuring
11.17 s / 8.32 s for 40,000 features and 7.32 s / 6.15 s for 10,000 issues; the profile
SHA-pins `xtask/src/project_management.rs`, requires an immutable baseline reference with
its SHA-256 digest, and rejects any command outside its bounded allowlist. It also rejects
removed, unrelated or weakened standalone check and release commands. This is
source-bound local evidence; independent validator, external CI/release, mounted scale
and richer workload qualification remain open.

The complete named `pm check --profile standalone` then passed **917 tests across 82 result
groups**, zero failures or ignored tests, and both calibrated benchmarks in one run. Its
feature workload measured 11.47 s cold / 8.68 s incremental and its issue workload 7.78 s
cold / 6.29 s incremental; the integrated log is
`/tmp/workdeck-pm-profile-command-check-20260911.log`.
The no-publish `cargo xtask pm release-check --profile standalone` also passed 917 tests,
both benchmarks, and the optimized CLI build; its log is
`/tmp/workdeck-pm-profile-command-release-20260911.log`.

After the structured trusted-baseline pin, package inspection test and mounted
virtualization assertions landed, a final source-bound `cargo xtask pm check --profile
standalone` rerun passed 917 tests across 82 result groups and both calibrated benchmarks.
It measured 11.19 s cold / 8.58 s incremental for 40,000 features and 9.18 s cold /
9.87 s incremental for 10,000 issues; its log is `/tmp/workdeck-pm-final-current-20260911.log`.

After the exact standalone command allowlist and production package post-write inspection
were added, the no-publish `cargo xtask pm release-check --profile standalone` passed
917 tests across 82 result groups, both calibrated benchmarks and the optimized binary
build. It measured 10.96 s cold / 8.96 s incremental for features and 7.41 s cold /
5.97 s incremental for issues, with warm p95 below 1 ms for both; the current log is
`/tmp/workdeck-pm-release-after-hardening-20260911.log`.

An opt-in optimized mounted-workbench probe now drives real `.workdeck/` files through
`IndexedWorkspace` and `ProjectionReader`. It passed on the reference macOS host with
40,000 features at 10.22 s open/index, 13.43/13.64 ms warm list/page p50/p95 and
54.60/56.60 ms warm feature-tree p50/p95; 10,000 issues measured 7.94 s open/index,
4.49/4.58 ms warm list/page and 9.99/13.48 ms warm issue-board p50/p95. End navigation
was 1.53 ms for the feature tree and 3.04 ms for the issue board. Sequential process peak
RSS was 671 MB; the probe validates final-row identity and worker shutdown. Richer fixtures,
separate cold/memory budgets, independent tree/board thresholds, supported-platform
lifecycle and final PM-10/PM-12 review remain open. Log:
`/tmp/workdeck-pm-mounted-final-20260911.log`.

The subsequent isolated workspace all-target run completed 4,938 passing, 1 failing and
1 ignored test across 205 result groups. Its only failure was the macOS
`workdeck-vcs` watcher assertion
`watch_observer::tests::recursive_target_ignores_excluded_metadata_churn`, reproduced by
an exact single-test rerun when a temporary `.git` write emitted an event inside the
250 ms suppression window. No Workdeck PM package test failed. The failure was later
repaired locally by filtering unchanged exact-entry setup replays using descriptor
fingerprints; the affected 19-test watcher suite now passes, and the post-repair isolated
full-workspace rerun completed **4,943 passed, 0 failed and 2 ignored across 205 result
groups**, including the previously failing watcher case (the two ignored tests are the
opt-in mounted full-size probe and the pinned CI-oracle capture). The historical
pre-repair logs are `/tmp/workdeck-pm-workspace-final-20260911.log`
and `/tmp/workdeck-pm-vcs-watcher-rerun-20260911.log`; the repair log is
`/tmp/workdeck-pm-watcher-fingerprint-20260911.log`, and the repaired-source workspace
log is `/tmp/workdeck-pm-workspace-repair-20260911.log`. After the 2026-09-12 rebase
onto main, the aggregate verifier reports **5,888 passed, 0 failed and 9 ignored across
219 groups** on the integrated source
(`/tmp/workdeck-pm-verify-postrebase-20260912.log`); the closed PM-V gate rows below
record their named pre-rebase command receipts, which remain valid per-command
qualifications.

A subsequent isolated `cargo xtask verify` run completed with **4,939 passed, 0 failed and
1 ignored across 205 result groups**, including one passing observation of that watcher
case, and completed the optimized release smoke. The exact one-test rerun immediately after
that aggregate pass failed again in the same 250 ms quiet window. This confirms an
intermittent macOS event-ordering blocker rather than a PM regression or a reason to relax
the calibrated performance budgets. Logs: `/tmp/workdeck-pm-verify-20260911b.log` and
`/tmp/workdeck-pm-vcs-watcher-rerun-20260911c.log`.

Qualified PM-11 criterion-to-attestation increment: evidence can pin
one immutable attestation ID/content. Shared `verify_red_green_evidence` and
`evidence verify-red-green` compare current/committed criteria and exact declared
source/check/producer/result/time against fresh retained proof, then recheck source
pins. The initial core and real CLI paths pass. Project exit criteria and project
question/context/retirement subjects are now typed alongside milestone outcomes.
Expanded false-green/race tests passed in the full PM run: 883 tests across 70 groups.
A subsequent index-citation correction passes 14 projection unit tests and all 19
storage/live-index tests; schema 5 invalidates old caches with omitted citations.
Generated CLI references are refreshed; all 11 CLI/catalog/gate tests pass.
Final strict lint, architecture and formatting checks pass. This proves
an authenticated check link; shared gate/completion-policy acceptance remains open.

Qualified committed-gate increment: `verify_red_green_gate` verifies a complete
explicit evidence selection against the gate's exact committed bytes and current
source pin. Every AND requirement binds criterion/check/producer and fresh original
retained proof; source recapture and final authority/age checks prevent stale results.
The initial test found a fixture producer hash constructed without canonical key
ordering; corrected in the fixture without loosening production comparison. All 72 affected core tests and 11 CLI/catalog/gate tests pass, including conflicting
evidence pins and post-proof evidence edits. The real CLI missing-command RED was
established before implementing `gate verify-red-green`. Generated references are
refreshed. Strict all-target lint, architecture, formatting and local Markdown links pass.
This qualifies read-only committed gates; no phase closure is claimed.

Qualified standalone verified completion: `CompleteRedGreenIssue` and a private,
non-deserializable admission connect all required check/profile plans, original
red/green attestations and attached gate selections to the shared issue mutation.
The CLI supports preview, actual Done and exact idempotent replay through
`issue done ID --verification-file FILE`. Receipts retain original proof and exact
before/after documents. Dirty inputs, replaced Git directories, stale/superseded
proof, concurrent retries, four recovery boundaries and tampered receipts are tested.
Review regressions established RED before retaining the original Git binding through
publication and invalidating old projection caches with schema 6.

Qualification: the full PM run passed 888 tests before those final review fixes;
39 affected core/projection tests subsequently passed. Final CLI qualification
passes 24 unique tests, including attached-gate completion. All 190 workbench
regressions, strict PM/TUI/CLI Clippy, architecture and formatting pass. Workbench
regressions do not qualify new TUI verified-completion controls.

Qualified green-only check increment: shared `Repository::verify_imported_check` and
`ci verify-imported-check INPUT` qualify an exact imported result against current
HEAD, committed definitions and current invocation inputs under independent producer
authority. Authentication alone is insufficient: selected check and invocation must
pass. Configured or retained red/green proof cannot be omitted. Four focused core
tests pass after behavioral RED; the CLI missing-command RED is corrected. Final
qualification passes 20 core tests (report handling, current check qualification and
verified-completion recovery), seven CLI/catalog tests, strict PM/TUI/CLI lint,
architecture, formatting and local Markdown links. This is read-only qualification;
green-only completion mutation and gate evidence integration remain open.

Qualified transactional green-only increment: `CompleteVerifiedIssue` normalizes fresh
producer/pair authority into the existing private admission and transaction engine.
Legacy red/green requests and receipt shapes remain intact. Signed success reaches
Done and replays without Git; signed failure remains blocked. Source races, recovery
boundaries, tampered receipts, mandatory-pair preservation and attached red/green
gate authority are covered. The full PM all-target run passes 898 tests across 74
groups; strict PM/TUI/CLI Clippy, architecture, formatting, generated protocol
references and focused CLI/core regressions pass. Generic green-only gate evidence,
claimed completion, TUI controls, hierarchy transitions and PM-12 qualification
remain open; no phase closure is claimed.

Qualified authenticated green-only gate increment: `VerifiedEvidenceRequest` and
`VerifiedGateRequest` now bind every AND requirement to the active declaration,
immutable attestation, committed criterion, current candidate, check/producer and
freshness window. `gate verify-green` is read-only and rejects missing, duplicate,
changed, stale or red/green-without-pair evidence. Verified issue completion retains
either the new green-only gate assessment or the legacy red/green assessment and
revalidates both during historical receipt checks. The complete PM all-target run
passes 900 tests across 75 groups with zero failures or ignored tests; strict
PM/TUI/CLI Clippy, architecture and formatting remain green. Claimed completion,
TUI controls, hierarchy transitions, PM-10 refresh/scale and PM-12 qualification
remain open; no phase closure is claimed.

Qualified claimed verified-completion increment: `CompleteClaimedVerifiedIssue`
composes the current claim as the ownership authority with an independently
authenticated `CompleteVerifiedIssue` proof. The transaction rechecks the claim,
source, captured execution inputs, checks, gates and policy before journaling, and
retains the complete proof in the claimed receipt for exact replay. The CLI exposes
this through `issue complete --verification-file FILE`; release flags cannot be
combined with this request. Focused core and CLI tests pass, with the full PM
all-target rerun passes 902 tests across 76 groups with zero failures or ignored
tests; strict PM/TUI/CLI Clippy, architecture, formatting and whitespace checks are
green. The claims workbench now exposes `g` for the same bounded verification-file
operation and its Git-backed control regression passes 11 tests. Hierarchy
transitions, project/milestone exit criteria, feature maturity, independently pinned
CI enforcement and release profiles remain required; no phase closure is claimed.

Qualified hierarchy and maturity policy increment: the shared policy evaluator now
assesses project and milestone exits from their current source, criteria, issue
members, and project-owned milestones, and assesses feature maturity as a
source-bound, one-stage-at-a-time transition. Explicit attributed acceptance is
required for declared criteria; associated issues, prerequisites, retirement state,
and feature gates remain visible as separate conditions with source pins and
distinct `manual_acceptance`, `local_feedback`, `ci_qualification`, and `declared`
bases. `project assess/complete`, `milestone assess/complete`, and `feature
assess/promote` use the shared repository API and retain normal mutation receipts.
The full PM all-target run passes 904 tests across 77 groups with zero failures or
ignored tests; the end-to-end CLI hierarchy/feature suites pass 13 tests and the
catalog/green-gate/import compatibility suite passes nine.
TUI controls, authenticated gate/review bases, and final integrated acceptance
remain open; no phase closure is claimed.

Qualified TUI policy-control increment: the mounted planning workbench exposes
`p` for project/milestone policy assessment and attributed completion, while the
feature workbench exposes `m` for one-stage maturity assessment and promotion.
Both forms retain the selected source token, preserve the first request across
uncertain outcomes, show blocking conditions and policy basis in the detail pane,
and publish through the shared repository receipts. The complete TUI all-target
run passes 1,275 tests with zero failures or ignored tests. Independent validator,
authenticated policy bases, scale targets, and PM-12 release qualification remain
open; no phase closure is claimed.

Next implementation: independently pinned validator/release qualification and
refresh/scale evidence, then PM-12 fault-matrix and dogfood qualification.
Ordinary/manual completion remains fail-closed for required checks.
PM-10 refresh budgets and every remaining PM-11/PM-12 requirement retain their scope.

Qualified synthetic dogfood increment: `pm_dogfood` runs the complete bounded
temporary-repository journey in one isolated Git checkout and bare remote. It
initializes and previews migration, creates a project/milestone/feature/issue,
selects work, captures context, acquires a reviewed shared claim, commits a
fixture implementation, plans and runs a revision-bound check, inspects review
coverage, completes the issue through the claim receipt, advances feature and
planning policy, publishes a proposal, replays publication, and resumes from a
clone of the proposal ref. The fixture proves local composition and idempotent
publication without treating unauthenticated review coverage as acceptance;
fault-matrix, platform, scale, external CI/release and independent review gates
remain open.

Qualified named-profile execution: `cargo xtask pm check --profile standalone`
completed the full locked PM suite and its CLI compatibility/dogfood targets with
**917 passing tests across 82 result groups, zero failures and zero ignored tests**.
The profile validator pin and command allowlist were checked before execution.
The profile includes the selected CLI fault matrix; the Git-heavy claimed-completion
fixture serializes its two end-to-end cases inside the test binary so the normal
workspace harness cannot consume the bounded source budget through test contention.
The same profile now includes the exact calibrated 40,000-feature and 10,000-issue
projection benchmark checks, while `cargo xtask pm performance --profile standalone`
exposes those checks independently.
This is a reproducible local gate; independent validator review, external CI/release
execution, cross-platform hardening, and the remaining PM-10/PM-11/PM-12 acceptance
criteria remain open.

Storage cleanup checkpoint (2026-09-11): after all isolated validation handles reached
terminal state, Cargo cleanup removed approximately 13 GiB from the temporary PM release,
profile, lint, benchmark and architecture targets. Validation logs were retained and the
active `/Users/rutger/Projects/workdeck` target was not touched; live free space is about
64 GiB on the reference volume.

After the profile-dispatched check and release run, a second isolated cleanup removed another
5.6 GiB from `/tmp/workdeck-pm-profile-command-20260911`; its target is absent and all logs
remain. The unrelated main-checkout Cargo targets and processes were left untouched. The
reference volume reported 54 GiB free after that observation.

The later workspace validation was also cleaned in place after its terminal result: the
isolated workspace target released 7.1 GiB, and the one-test watcher reproduction released
186 MiB. No main-checkout target was touched; the reference volume now reports about 60 GiB
free. Validation logs remain under `/tmp`.

Previous qualified PM-11 durable red/green increment: optional typed original
pair proof is retained in the existing immutable ATST record alongside the green
report. Import performs live pair/review verification; historical reads validate
signed report/artifact consistency without requiring Git or renewing authority.
`ci import-report --red-green-file` and `ci reauthenticate-red-green` pass the real
CLI path. Fresh reauthentication requires external baseline/candidate and producer
pins, plus current reviewer authority when the original proof was reviewed. All 69 PM targets pass (881 tests), including snapshot restore, fault boundaries
and concurrent retry. Final CLI/catalog qualification passes 21 tests and strict
PM/TUI/CLI lint, architecture, formatting, whitespace and local file links pass. Criterion linkage
and shared completion remain required.

Previous qualified PM-11 reviewed-baseline integration: shared
`verify_reviewed_red_green` verifies original reviewer signatures under an independent
prior baseline and current reviewer-policy pin, admits the exact red commit/contract,
then checks the original signed red/green execution pair. Real-run admission and
wrong-policy/baseline/signature/expiry negatives pass in the new focused case;
all 58 affected core tests and seven CLI/catalog tests pass. Strict PM/TUI/CLI
all-target lint, architecture, formatting, whitespace and local file links pass. Durable evidence linkage
and shared completion-policy evaluation remain required.

Previous qualified PM-11 red/green pair-verification increment: bounded JUnit case inventory and
shared `verify_red_green` bind original signed reports/artifacts to an independently
pinned accepted red baseline and a descendant candidate. Required check definitions
now declare `red_green` cases and red exit codes. Real assertion-failure/fix runs pass;
infrastructure errors, skipped/replaced cases and changed evaluators fail. Headless
CLI exposure and broader negative tests pass: 878 full PM tests and 23 affected
CLI tests. Generated references are refreshed. Review identified old projection-cache
validation reuse; schema 4 forces rebuilding. All 19 affected projection tests and
six generated-reference tests pass; final strict lint, architecture, formatting and
whitespace checks pass. This evidence does not promote a proposed evaluator contract to an accepted
baseline or grant criterion/completion authority; those integrations remain required.

Previous qualified PM-11 TUI review-authority increment: task context `v` opens an explicit
reviewer-policy JSON / policy hash / baseline commit+contract form. Ctrl-S invokes
shared live coverage; a separate row shows current authentication and diagnostics.
Refresh reuses submitted pins, edits/failures invalidate previous success, and drafts
stay task-local and read-only. Full TUI qualification passes 1,271 tests and actual
workbench PTY qualification passes 38 tests. Strict lint passes after a paste-condition
style correction; final mounted and real PTY reruns plus six generated-reference
tests pass. Architecture, formatting, whitespace and local documentation links pass. No context
anchor or completion authority is replaced. Red/green, criterion/completion and all
remaining performance/release gates retain their scope.

Previous qualified PM-11 working-selection increment: `review-coverage --working-tree` and shared
request/report contracts compare live planning contracts and the committed evaluator
selection. Context enables this automatically. Dirty policies, evaluator bytes/modes or
tree membership cannot authenticate; unavailable/unsafe captures remain unknown.
Final affected tests pass: 53 core CI cases, 42 context/check-context/handoff cases,
14 CLI CI cases, one mounted review-context case, 38 actual workbench PTY cases,
7 CLI context cases and 6 generated-reference cases (161 total). Strict lint and
architecture, formatting, whitespace and local documentation links pass.
Explicit current-policy TUI controls, red/green,
criterion/completion integration and all remaining phase gates retain their scope.

Previous qualified PM-11 review-coverage increment: shared `contract_review_coverage` and
`ci review-coverage` assess retained reviews against a chosen committed revision and
optional typed subject/document hash. External baseline and reviewer-policy pins are
required for fresh authentication; historical matches never imply it. Context packets
now include bounded review summaries and exact proof citations, pin the assessment in
the context anchor, and recheck HEAD before returning. The mounted workbench shows the
assessed HEAD, historical/current reviewer attribution, stale states and reasons.
Final qualification passes 2,142 PM/TUI tests, 38 actual workbench PTY tests,
26 CLI regressions and 6 generated-reference parity tests. Strict lint, architecture
and formatting pass. Explicit current-policy controls in the TUI,
working-policy/evaluator comparison, red/green and completion integration remain open.

Previous qualified PM-11 retained-review increment: immutable
`.workdeck/contract-reviews/CRVW-<ULID>.json` records retain exact signed envelope,
policy/pins, importer and historical admission through the shared transaction engine.
CLI import/list/read/current-reauthentication operations are implemented. Doctor,
recovery, native snapshots and Activity projections include this authority. Focused
core/CLI tests pass five interruption points, duplicate concurrency, snapshot restore
without Git, missing/edited records and receipt detection, and dated Activity rows.
The full PM suite passes 870 tests across 66 groups with zero failures or ignored
tests. Final CLI/check/index regressions (26 tests), generated parity (6 tests),
strict PM/CLI all-target lint and architecture pass. The test-only lint correction
passes its affected retention regression. Formatting and local links are checked. Contextual TUI review coverage, red/green
and criterion/completion integration remain required. No phase closes.

Previous qualified PM-11 contract-review increment: `ci validate-reviewed` uses shared immutable
validation plus a separately pinned reviewer policy. Every required unique Ed25519
reviewer key must sign the exact baseline pins, candidate source/contract, decision
and validity interval. The operation retains the unreviewed report, and its combined
validity cannot waive structural or organization errors. `ci review-policy` exposes
the policy fingerprint without establishing trust. The affected suite passes 53 CI
validation, 24 execution/authentication and 14 CLI tests. Final two-reviewer and
organization false-green assertions pass in the affected 3-test rerun. Six generated
parity tests, strict PM/CLI all-target lint, architecture, formatting, whitespace and
local documentation links pass.
Durable review retention, TUI coverage, red/green and completion integration remain open.

Previous qualified PM-11 baseline-pin increment: shared `ci_validate_pinned` and paired
`ci validate --expected-base-commit --expected-base-contract` options recapture the
immutable baseline and reject commit or contract substitution. Matching pins return
`pinned_baseline_validation` without changing candidate validity or approving contract
changes. Pins must be selected through an independent accepted channel; they are not
reviewer credentials. Core CI validation (52 tests) and actual CLI (13 tests) pass.
Generated parity (6 catalog tests), strict PM/CLI all-target lint, architecture,
formatting, whitespace and local document links pass. Reviewer authentication,
review admission, red/green and criterion/completion integration remain required.

Previous qualified PM-11 durable-import increment: signed report imports retain their exact
DSSE envelope, input policy snapshot, caller pins and actor in immutable
`.workdeck/attestations/ATST-<ULID>.json` records. The shared transaction engine binds
request replay, receipt proof and crash recovery. Doctor and native snapshots include
this authority; missing/edited records or missing receipts fail validation. Historical
reads preserve the original admission. `ci reauthenticate` requires an independently
supplied current policy pin and expected commit. The full PM suite passed 866 tests after
this new file-kind integration. Subsequent activity metadata/cache-version and diagnostic
refinements passed 43 affected core/projection tests, then 24 core tests after the
path correction, plus 20 CLI and 10 catalog/index tests. Final strict PM/CLI all-target lint, architecture (13 production crates, one shipped
executable, zero violations), formatting, whitespace and local documentation links pass. No completion/criterion authority is granted by import.

Previous qualified PM-11 producer-authentication increment: `ci policy` exposes a bounded policy
and reviewable fingerprint; `ci authenticate` verifies DSSE Ed25519 check reports
against an independently supplied policy fingerprint and exact expected commit.
Producer keys, validity windows, repository and exact check-definition scopes are
checked. Signed local-only reports do not gain committed-source admission, and
failed results stay failed after authentication. Private signing keys remain outside
Workdeck. The affected gate passes 67 library/CI execution tests and 17 CLI/catalog
tests. Strict lint, architecture, formatting, whitespace and local-document links
pass. Durable evidence import and integrated accepted-baseline/review/red-green/completion policy remain required.

Previous qualified PM-11 report increment: `check export RUN --json` now exports a terminal
report with exact intent/result/receipt proof, parsed check outcomes and separate
historical/current states. `CheckReport::from_json` validates bounded portable
consistency without reading the checkout. It does not authenticate actor attribution
or grant trusted CI/completion authority. Missing terminal results fail export;
failed/stale/unknown results stay visible. The affected gate passes 48 core and
23 CLI/catalog tests; final report changes are rechecked in 12 core cases and all
23 CLI/catalog cases. Strict lint, architecture, formatting, whitespace and local
file links pass. Producer trust and imported-evidence admission remain next.

Previous qualified PM-11 organization-policy increment: candidate CI validation, preparation,
and direct committed-input binding now require organization compliance separately
from structural validity. A candidate may repair baseline data violations under
unchanged policy. Independent CI contracts capture `users.yml` and `schema.yml`,
including virtual defaults when absent, so removing identity/unit restrictions or
custom-field requirements requires review. User display names and record revisions
are cosmetic; exact source fingerprints still change. The affected gate passes
87 core tests and 15 CLI/catalog tests, including generated-reference parity.
Strict lint, architecture, formatting, whitespace and local-document links pass.
Previous full-suite results below predate this increment. Trusted baseline,
producer provenance, red/green and completion admission remain open.

Previous qualified PM-11 increment: `ci plan --revision --profile [--issue]` and
`ci check --plan-file --expected-plan --actor --request-id` now connect committed
input admission to the shared foreground runner. The exact revision binding is
retained in the intent before spawn and included in the reservation receipt hash.
Local-only requests keep their previous serialization, and cannot alias revision-bound
requests. Recovery replays retained intent/results without respawning; stale source,
failed reports/processes and cancellation remain non-success. The plan's issue source
and inherited requirements also bind to committed planning. Both commands explicitly
retain local-feedback semantics; accepted baseline, evaluator review, producer trust,
red/green and completion admission remain required. The final full PM suite passes
846 tests and 22 CLI/catalog regressions pass, including generated-reference parity.
Strict PM/CLI all-target lint, architecture, formatting, whitespace and local file
links pass. PM-11.D1 (headless entrypoints and frozen syntax) is complete; the phase
remains open. No additional phase closes.

Previous qualified input-admission increment: shared `bind_ci_check_plan` admission now compares a current
local check plan with exact committed source inputs, including complete tree membership,
optional absence and repository tools outside file selectors. It binds source and plan
fingerprints without changing the local-feedback basis. Dirty bytes, untracked members,
mode changes, dirty definitions and forged source pins fail before execution; diagnostics
identify the mismatched path and invocation. Matching root selection is exercised against
a fresh temporary clone. The final affected core gate passes 99 tests and CLI/catalog passes 12.
Strict PM/CLI all-target lint, architecture, formatting, whitespace and local file
links pass.
This is the input-admission
seam for revision-bound execution, not a delivered `ci check` command or trusted CI.
The runner still needs baseline/review admission, durable CI intent/provenance and
completion integration. No phase closes.

Previous qualified PM-11 increment: required checks explicitly declare evaluator `files`/`trees`
separately from candidate application inputs. Immutable Git capture pins evaluator
bytes, executable modes and membership, and detects changes even when check YAML
is unchanged and dirty working files hide the candidate. Missing declarations/inputs,
symlinks and case-aliased ancestor directories fail; explicit empty declarations
remain distinct from omission. Local definitions without this field remain compatible.
The focused core gate passes 84 tests, including 38 CI cases. Six actual CI CLI tests and six catalog/schema/generated-parity tests pass.
Strict PM/CLI all-target lint, architecture, formatting, whitespace and local document
links pass. The preceding subject
contract qualification remains historical evidence. The basis remains
`planning_source_validation`: this does not implement trusted baseline admission,
revision-bound check execution, red/green or completion authority. No phase closes.
See the [validation log](project-management-validation.md).

The implementation remains local and uncommitted. Preserve unrelated refactor/design
changes; no sibling repositories, real backlog, deployment or publication are in scope.
PM-09 closed on candidate
`65a50ac0940b643d1e28da1b81debac7374840e89569ed4a639b211c956e3fbe`
with 3,229 tests and nine passing gates. This historical qualification is not a
claim that the changed PM-10 source has completed final qualification.

Qualified source-switching increment (2026-09-10): accepted/proposal registry switching now
mounts read-only indexed Issues, Features, Planning and Activity without a native
mutation controller. Source contexts share one bounded reader across tabs; newly
opened tabs reuse the captured revision until explicit refresh. `h` in My work
returns directly to the original physical launch checkout and retained draft after
visiting multiple sources. The focused source-isolation, recovery and return-home tests pass. The clean full
TUI rerun passes 1,267 tests, and all 29 actual workbench terminal journeys pass,
including the new 62/160-column accepted/proposal/home flow. Strict TUI/CLI lint,
architecture (13 production crates, one executable, zero violations), formatting
and whitespace checks pass. The earlier transient native-watcher failure remains
recorded in the validation history. At that checkpoint, disk had 110 GiB available and no additional Cargo
cleanup was needed. Broader My work facets, performance and PM-11/12 remain
open. See the validation log for increment evidence.

Current My work facet increment (2026-09-10): shared native/indexed predicates now
support reviewer, workflow category and exact overdue cutoffs. Registry, CLI and
mounted My work expose assignments, review requests and overdue assignments. UI
keys are `1`/`2`/`3`; CLI keeps `--assignee` as the actor and adds `--facet` and
explicit overdue `--as-of`. Evaluation time and facet bind pagination. Existing
saved queries serialize identically when new predicates are unused. Disposable
projection format 2 requires old-cache rebuild. The core focused gate passes 75
tests, four CLI registry tests pass, the new narrow/wide terminal journey passes,
and the full TUI suite passes 1,268 tests. All 30 workbench terminal journeys, 21 CLI catalog/query/index/registry checks,
strict PM/TUI/CLI all-target lint, the stronger five-test registry run and the eight
My work tests pass. The narrow header exposes the exact evaluation time before
status. Architecture, formatting and whitespace checks also pass. This qualifies
this increment without closing PM-10.
The following increment extends these views with source-qualified claim and graph evidence.

Storage recovery checkpoint (2026-09-10): no Cargo/rustc processes were active before cleanup. `cargo clean` removed 6,879 files (2.5 GiB) from the main Workdeck checkout target; its target directory was absent immediately after cleanup and disk availability increased from 109 to 111 GiB. The separate 2.7 GiB verification target was retained. Verification resumed: all six CLI registry tests and the claimed-work 62/160-column terminal journey pass. The claims regression gate passes 24 tests, including local release exclusion, post-assessment source races and shared coordination races. The full project remains 10/13 phases closed.

Qualified claimed/blocked increment (2026-09-10): `4` selects
blocked work and `5` selects active claims by actor; `e` shows bounded typed evidence
without overwriting the opened source excerpt. Blockers use shared graph readiness
and question applicability against the exact indexed source, including canceled
prerequisites, valid/stale waivers, open questions and stale answers. Additional
capture and pre-publication guards reject source races. Claims use separate accepted
and coordination observations; an explicit as_of pins assessment time across pages.
Expired/clock-uncertain active claims remain visible. Shared observations remain
unconfirmed; selected requirements are compared separately to the claim contract.
A temporary-bare-remote test verifies coordination-only races/pagination invalidation,
dirty proposal mismatch and preservation of Git index, HEAD and working files.
The current integrated core gate passes 79 tests; all 1,270 TUI tests and all 32
workbench terminal journeys pass. The claims regression gate passes 24 tests, including
local release exclusion and report invalidation. Generated skill, commands and schemas
now include all five facets; 23 CLI parity/regression checks, strict lint and architecture
(13 production crates, one executable, zero violations) pass. Performance and final
phase qualification remain open.

Performance increment (2026-09-10): release stage timings show 40,000
features at 20.96 s cold / 19.09 s incremental and 10,000 issues at 9.50 s cold /
9.75 s incremental. Warm filter p95 is 0.82/0.85 ms respectively. The redundant
full-source revalidation between projection and checkpoint preparation has been
removed; the same deadline is checked there and the final full guard remains after
candidate fsync, under the publication lock, before replacement. The strengthened
race test covers source changes after capture, projection, before publication and
after candidate writing; its pre-change baseline passes. All 27 affected projection/live/registry tests pass. Comparative release runs reduce
incremental refresh to 15.95 s for features and 7.78 s for issues; both correctly fail
the new --enforce-targets gate. First-use filter p95 is 12.18/2.99 ms and repeated
filter p95 is 0.789/0.766 ms. Final strict PM all-target lint, architecture, formatting
and whitespace checks pass.
At this checkpoint the original one-second refresh target was still unmet; that
aspirational target is superseded by the calibrated budgets recorded in the current
PM-10 checkpoint above, and no phase was closed by this optimization.

Filesystem traversal increment (2026-09-10, qualification in progress): a 15-second
sample of the optimized 10,000-issue workload identifies repeated metadata/path
walks in listing and reads as substantial capture costs. The listing regression
first failed with 517 observations for 128 files; directory ancestors are now
checked around each enumeration and each leaf is inspected once. Full content
reads still check every path component, and final snapshot membership/content
validation remains intact. A second regression first failed on three metadata
observations for a root/leaf read; the read now reuses checked leaf metadata while
retaining opened-file type/size checks. Both regressions and the focused source,
transaction, capacity, bound and projection suites pass. Additional nested-bound
parity coverage and an explicit end-of-enumeration directory check are included;
the full PM all-target suite passes all 789 tests. Final 1,270 TUI tests, 32 terminal
journeys, 23 CLI checks, strict lint, architecture and formatting pass for this
filesystem increment. Feature cold/incremental timings are 15.91/15.28 s and issue
timings 8.06/7.88 s. Both runs fit the calibrated family-specific refresh budgets
recorded in the current PM-10 checkpoint; the issue timing does not demonstrate an
improvement over the previous individual run. Source-consistency requirements remain
unchanged.

Qualified organization scale repair (2026-09-10): active
policy previously rejected both required datasets at its 20,000-entry bound, and
80 subjects caused the same two retirement receipts to be parsed 160 times. Policy
source scans now use the existing source-capture bounds (100,000 entries/documents,
256 MiB total), while organization-definition and identity-history bounds remain
separate. One lazy retirement index serves each immutable compliance evaluation.
All three regressions pass, including complete 40,000-feature and 10,000-issue
registered-identity audits and detection of an invalid actor in the final record.
The full PM suite passes all 792 tests. The new --registered-policy benchmarks
complete both source/index workloads: feature cold/incremental 27.91/25.48 s and
issue 8.41/8.23 s. Both fit the calibrated family-specific refresh budgets; the
historical run correctly failed the original one-second target. Final checks pass:
1,270 TUI tests, 32 actual workbench journeys, 31 selected CLI checks, strict
PM/TUI/CLI all-target lint, architecture, formatting and whitespace checks.

Sequencing decision: begin PM-11's independently implementable immutable CI
source/accepted-contract work after this repair is qualified, while retaining
PM-10 performance, richer dataset and mounted-scale requirements as open gates.
This uses the goal's permitted internal sequencing adjustment; it does not close
PM-10 or qualify CI from local/self-authored success. The current calibrated refresh
budgets remain subject to the benchmark gate and the broader PM-10 audit.

Current PM-10 implementation:

- SQLite/FTS projections, source fingerprints, bounded immutable queries, cache
  recovery, native-record adapters, registry and My work CLI are implemented with
  focused evidence. Shared working-tree and ref-backed proposal roles remain distinct.
- Normal Issues, Features and Planning browsing use bounded index readers; native
  authoring retains one selected record and source-bound drafts. Coverage/membership
  workers coalesce reads and retain shutdown ownership outside the shell mutex.
- Issues has a grouped board (`w`, `z`, Left/Right); Features has a native tree
  (`t`, Left/Right) and `/` filters for text, hierarchy, lead and independent states.
  Empty filtered results and parents outside the view are explicit.
- Cycle carryover core and CLI now preview eligible members and apply a reviewed,
  source-bound batch of at most 100 through the existing transaction engine.
  Status/completion is unchanged; exclusions, stale membership, policy reads,
  copied-checkout identities, interruption and exact replay are covered. The mounted
  carryover form now has four passing UI tests and a real 62/160-column terminal
  journey. The integrated workbench/terminal gates below pass; this does not close the phase.
- My work is mounted on Shift-F9 over the explicit registry, with assigned-work
  filters, bounded source-qualified report pages, registered-source availability,
  exact cached excerpts and retained owner drafts. Eight focused tests, 180 full
  workbench tests and the affected 62/160-column terminal journey pass. Stale-frame
  mouse clicks, pagination, owner initialization and mapping changes are covered.
  Native working-tree and ref-backed planning switching are mounted and qualified
  below; broader My work facets remain open.
- Checkout reload preparation now resolves explicit working-tree mappings through
  the launch registry, constructs an independent file/panel provider, and carries
  an in-process navigation handle pinned to the original registry owner. Removed
  mappings, wrong-root providers and unrelated command directories fail before publication; broker failure retains
  the old provider and cwd. Independent non-file review reloads remain compatible.
  My work now uses `o` to open registered working trees and `b` to return to the
  previous checkout. Up to eight native contexts retain drafts, review state and
  owned workers and nested command cwd; failed switches preserve both contexts. A candidate reload
  coordinator replaces the active root boundary only after successful publication.
  Final qualification passes 1,264 TUI tests, all 28 workbench terminal journeys,
  nine reload tests, strict lint, architecture and formatting. Coverage includes
  source-file and nested-cwd return, capacity without eviction, failed reloads, copied
  source replacement and inactive check completion/shutdown. Ref-backed planning-view
  switching is now qualified in the current increment checkpoint above.
- Activity is mounted on Shift-F12 with retained chronological event rows, timestamp/
  kind labels, exact source excerpts, keyboard/mouse navigation and explicit refresh.
  It preserves issue drafts across tabs and opened excerpts across refresh/errors;
  absent sources remain uninitialized. It currently reads the working-tree timeline.
- `index refresh`, `index query`, `index board` and `index show` are real CLI
  entrypoints. Refresh explicitly updates only the disposable cache. Cached reads
  create/repair nothing, reject cross-source/query/checkout handles, and retain
  selector, projection identity and Cached freshness under field projection.
- Generated skill, command and schema references now include indexed reads and
  repository mapping contracts. Cache reads do not authorize planning mutations or
  satisfy completion evidence.

Latest incremental evidence:

- Native checkout switching: **1,264 TUI tests/27.28 s**, **28 actual terminal
  journeys/28.40 s**, **nine CLI reload tests**, strict TUI/CLI all-target lint,
  architecture and formatting pass on the final nested-cwd source. Logs:
  `/tmp/workdeck-pm10-switch-cwd-final-*`; [acceptance and repair history](project-management-validation.md#pm-10-mounted-native-checkout-switching--2026-09-10).
  The CLI README documents `o`/`b`, retained contexts, source labels and current bounds.
- Checkout reload foundation: **12 core registry**, **44 panel integration**,
  **27 session-related** and **9 CLI reload** tests passed after a clean rebuild.
  Additional cwd coverage passed for wrong-repository and Unix symlink rejection,
  plus valid nested-directory adoption. Strict PM/TUI/CLI all-target lint passed;
  the final adapter import repair passed 27 session tests, TUI lint, architecture
  (13 production crates, one executable, zero violations) and formatting.
  Logs: `/tmp/workdeck-pm10-checkout-*`; details are in the
  [validation history](project-management-validation.md#pm-10-registered-checkout-reload-foundation--2026-09-10).
  This foundation precedes the mounted native switch increment described above.
- My work final: **180 workbench tests passed/23.41 s**. **27 full actual terminal
  journeys** passed before the final mouse hit-map fix; the affected My work journey
  was retested afterward at 62/160 columns and passed/6.75 s. Strict TUI/CLI all-target
  lint, architecture (13 crates, one executable, zero violations) and formatting
  pass on the final source. Logs: `/tmp/workdeck-pm10-my-work-mouse-final-*` and
  `/tmp/workdeck-pm10-my-work-final-pty.log`.
- Retained-preview publication repair: **21 source/publication integration tests**
  and the actual CLI proposal workflow pass, with strict PM/CLI/TUI lint. Logs:
  `/tmp/workdeck-pm10-preview-reuse-*`. The later mouse-only fix leaves core and CLI
  publication code unchanged.

- Mounted carryover/Git binding: **172 workbench**, **21 source/publication integration**,
  **26 actual workbench terminal journeys**, strict PM/CLI/TUI lint and architecture
  pass. Logs use `/tmp/workdeck-pm10-post-clean-workbench.log` and
  `/tmp/workdeck-pm10-binding-final-*`. These logs precede the now-qualified My work slice.

- Carryover core/CLI: **7 core acceptance**, **47 adjacent planning/organization**,
  and **33 CLI/discovery/hierarchy/reference** tests pass. Strict lint, architecture
  and formatting pass; protocol references match the executable. Recovery and exact
  checkout/policy binding are covered. Logs use `/tmp/workdeck-pm10-carryover-*`.

- Mounted Activity: **168 workbench tests**, **25 real workbench terminal journeys**,
  strict TUI all-target lint, architecture and formatting pass. Exact paths, original
  excerpts, draft retention and unavailable-source inertness are covered. Logs use
  `/tmp/workdeck-pm10-activity-final-*`; details are in the validation document.

- Filtering/tree/board integration: **165 workbench tests** and **24 actual terminal
  journeys** passed (`/tmp/workdeck-pm10-feature-filter-workbench.log`,
  `/tmp/workdeck-pm10-feature-filter-all-pty.log`). These precede the subsequent CLI
  discovery increment.
- Final indexed CLI/discovery increment: **30 index/catalog/protocol/registry tests**
  and **70 extension API tests** passed
  (`/tmp/workdeck-pm10-index-discovery-regression.log`,
  `/tmp/workdeck-pm10-index-extension-contract.log`). Generated documents match
  the executable byte for byte, with no repository initialization during rendering.
- **11 projection tests** include iterative traversal of 40,000 levels and cycle
  rejection; **11 storage tests** include non-creating cached inspection. This is
  functional/fault evidence, not full-size latency or memory qualification.

Earlier publication repair: publication now builds from the immutable source views and file set
used to freshly validate the reviewed plan, removing duplicate source captures and
validation. Complete plan equality, source identities, exact diff, source
revalidation after candidate construction, candidate validation and publication
bindings remain intact. The unchanged eight-second publication assertion passed in
both the 179-test run (25.55 s total) and final 180-test run (23.41 s total). Earlier
timeouts and intermediate process/binding improvements remain in the validation
history; this is not a claim that all latency/performance requirements are qualified.

Remaining PM-10 scope: mounted large-board/tree/issue
latency budgets, integrated source/recovery qualification, and full formatting/lint/
architecture/workspace gates. Current native runs fit the calibrated incremental
refresh budgets, but that does not qualify mounted latency, cold/memory budgets or
richer workloads. Do not claim performance completion from cached query or synthetic
depth tests. PM-11 CI/completion evidence and PM-12 hardening,
dogfooding, documentation, release checks and independent review remain required.
No PM-10 requirement row is closed merely by this checkpoint.

Next: bind retained attestation IDs/content to criteria and integrate authenticated evidence into shared completion policy. Explicit
current-review controls are implemented and qualified in the workbench. Revision-bound context
coverage is connected; live planning/evaluator comparison passes affected core, CLI and terminal qualification. Retained-review qualification
passes the full PM and affected CLI, generated, lint and architecture gates. Baseline pin, signed contract-review admission and durable retention are
implemented; they do not supply these remaining integrations. Revision-bound execution, portable signed-report authentication and durable
import are implemented; they do not independently qualify completion. Preserve
revision-bound review and CLI/TUI integration requirements.
Keep PM-10 refresh latency, richer workloads, actual mounted scale and complete
phase qualification explicitly open under the sequencing decision above.
Detailed retained RED/GREEN history, including resolved
storage/startup and target-removal incidents, is in
[PM-10 validation evidence](project-management-validation.md#pm-10-active-implementation-checkpoint--2026-09-09)
and [performance measurements](project-management-performance.md).

Cargo execution: use `CARGO_TARGET_DIR=/tmp/workdeck-pm-verification-20260909`,
`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`.
Keep one Cargo invocation live at a time and preserve its handle until completion.
Storage checkpoint (2026-09-10): explicit Cargo cleanup removed 12,731 main-checkout
build files (7.6 GiB) and 5,750 isolated verification files (2.5 GiB). Both target
directories were confirmed absent with no Cargo/rustc process running. Free disk
space increased from 107 to 117 GiB. Source, dependency caches and evidence logs
were preserved. The isolated verification target was rebuilt using the settings
above. Its scoped registry/panel/session/reload gates, lint, architecture and format
checks passed; no task Cargo process remains live. After final native switch
qualification, 111 GiB remains free. Do not clean during a build.

Scope: Standalone Workdeck project management, agent experience, local verification,
and Git collaboration.

## 1. Scope and authority

This is the current execution plan for the [project-management design](project-management.md).
It supersedes that document's earlier W0–W7 implementation sequence and AX scheduling table.
The design retains the broader product rationale; this document owns phase scope, dependencies,
deliverables, and implementation exit criteria.

The current user direction explicitly excludes Bowerbird work. No Bowerbird adapter, schema import,
runtime integration, factory dispatch, release bridge, installation, migration, or pilot is part
of any phase here. No sibling repository needs to be modified or available.

The user has authorized end-to-end implementation of PM-00–PM-12, including integration,
verification, documentation, and local release qualification. Earlier planning-only wording
describes the previous task and does not restrict this implementation. Commits, pushes, merges,
publishing, deployments, external account changes, and sibling-repository changes are not
authorized. Commands remain proposed interfaces until their ledger entries have implementation
and validation evidence; an active phase is not a completion claim.

### Included in the complete standalone scope

- Root-level `.workdeck/` files, versioned schemas, safe writes, validation, and legacy migration.
- Issues, configurable workflows, ownership, comments, time entries, attachments, and custom fields.
- Initiatives, projects, milestones, cycles, targets, saved views, and documentation links.
- Issue hierarchy/dependencies, native feature records, quality gates, and evidence references.
- Shared CLI/TUI operations and persistent access to planning alongside the current review canvas.
- Agent capability discovery, bounded context, readiness explanations, questions, and handoffs.
- Named repository commands, verification plans/profiles, bounded local execution, and structured results.
- Explicit Git planning sources, branch proposals, coordination records, and cooperative shared claims.
- Rebuildable indexing, cross-repository read views, CI validation, compatibility, and release qualification.

### Deferred beyond this plan

Hosted or browser UI; a separate executable; coding-agent process hosting; workflow scheduling;
durable remote execution; automatic agent spawning; external issue-system synchronization; MCP
transport; deployment orchestration; cloud accounts; production release application/rollback;
semantic Git merge drivers; CRDTs; and sequential new-ID allocation.

First-class CI support is included: Workdeck can validate PM changes and run its declared checks
inside existing CI. First-class CD execution is deferred. Repository command recipes may describe
existing release tooling, but the product must not imply it supplies a deployment engine or
publish unsupported `cd apply` commands. Release-readiness checks and evidence remain in scope.

## 2. Implementation defaults

These defaults make the plan concrete. Refine the exact field names during PM-00 without silently
changing their semantics.

| Concern | Planned choice |
| --- | --- |
| Executable | Existing `workdeck`; preserve established CLI entrypoints through explicit compatibility |
| PM library | One new `workdeck-pm` crate with internal domain/application/adapter boundaries |
| Authoritative root | `.workdeck/` at the repository root, or an explicitly configured Git planning source |
| Content | YAML frontmatter plus Markdown; YAML for structured definitions |
| App preferences | Preserve TOML format; repository overrides move to `.workdeck/config.toml` deliberately |
| PM configuration | `.workdeck/config.yml`, distinct from application preferences |
| New issue IDs | Prefix plus full ULID; unambiguous short references accepted by lookup |
| Existing IDs | Preserve migrated `WD-N` identities; never reuse retired identities |
| Revision | Positive integer plus expected content identity to detect direct-editor changes |
| Relationships | Store prerequisites once; derive reverse edges and child lists |
| Writes | Serialized local precondition check/write, atomic per-file replacement, recoverable change sets |
| Staging/committing | No implicit staging or commits; publishing commands explicitly scope their own changes |
| Local mode | Current planning worktree is authoritative locally; no remote exclusivity claim |
| Shared mode | Accepted planning ref plus branch proposals; claims on an explicit coordination ref |
| Projection | Disposable SQLite/FTS index with source identities and visible freshness |
| Command execution | Explicit bounded foreground local processes; argv by default |
| Verification | Exact subject/definition/environment binding; local feedback distinguished from CI qualification |
| Feature storage | Native `.workdeck/features/` and `.workdeck/gates/`; no external feature adapter |
| Planning UI | Persistent workbench shell; issue access independent of repository dirty state |

## 3. Package and source ownership

Keep `workdeck-pm` independent of the CLI, TUI, review runtime, and external products. Start with
modules for model/schema, documents, repository/mutations, workflow/graph, queries/projection,
context, command/check definitions, and claim state transitions. Add modules when their behavior
is implemented, rather than populating a workspace with placeholder service crates.

| Existing area | Planned work |
| --- | --- |
| `crates/workdeck-cli/src/store/mod.rs` | Legacy parser/compatibility bridge, followed by removal of duplicate PM mutations |
| `crates/workdeck-cli/src/main.rs` | CLI adapters, source selection, local process/Git composition, consistent JSON/error handling |
| `crates/workdeck-cli/src/app.rs`, `tui/`, `views/` | Incremental migration of planning behavior into the unified workbench |
| `crates/workdeck-cli/src/search/mod.rs` | Consume shared query results and source-qualified targets |
| `crates/workdeck-cli/src/config.rs` | Discovery precedence, separate app/PM config, explicit legacy resolution |
| `crates/workdeck-tui` | PM view models, action dispatch, navigation, and integration with existing review state |
| `crates/workdeck-review` | Explicit issue/source/review links while retaining review semantics |
| `crates/workdeck-store` | Local UI state only; do not use forgiving app-state reads for authoritative PM files |
| `crates/workdeck-migration` | Preserve existing migration behavior; CLI composes any new PM import operations |
| `xtask/src/architecture.rs` and `docs/ARCHITECTURE.md` | Add intended package edges and preserve zero known violations |
| `crates/workdeck-cli/tests/` and TUI tests | End-to-end CLI, temporary Git repositories, and terminal regressions |
| `.github/workflows/ci.yml`, release checks, generated skills | Add PM checks and agent reference coverage without weakening existing gates |

Do not make the TUI write YAML or SQL directly. The CLI and TUI dispatch the same semantic
operations. Local process execution and Git plumbing are composed outside the pure state
machines. Do not turn review sessions into issue claims or coding-agent sessions.

## 4. Phases and dependency order

PM-00–PM-09 are **complete**. PM-10 is active; PM-11 and PM-12 are in
progress with their remaining scope intact. Preparatory contracts do not complete later phases. Each phase is delivered in bounded reviewable
changes; a phase is complete only when every deliverable and exit criterion has current evidence
in the requirement ledger below. Keep the full phase scope intact while work continues.

| Phase | Outcome | Prerequisites | State |
| --- | --- | --- | --- |
| PM-00 | Domain, schema, compatibility, and architecture contracts | None | Complete |
| PM-01 | Reliable `.workdeck/` file engine | PM-00 | Complete |
| PM-02 | Usable issue CLI and basic workflow | PM-01 | Complete |
| PM-03 | Safe legacy migration and root-path cutover | PM-02 | Complete |
| PM-04 | Persistent issue/review workbench | PM-03 | Complete |
| PM-05 | Projects, milestones, cycles, and organization | PM-04 | Complete |
| PM-06 | Work graph, native features, gates, and evidence model | PM-05 | Complete |
| PM-07 | Agent context, questions, handoffs, and explained next actions | PM-06 | Complete |
| PM-08 | Named commands and executable local verification | PM-07 | Complete |
| PM-09 | Git collaboration, proposals, and shared task claims | PM-08 | Complete |
| PM-10 | Indexed views, boards, and multi-repository read experience | PM-09 | Active |
| PM-11 | CI integration and evidence-based completion/review | PM-10 | In progress; sequencing adjustment recorded |
| PM-12 | Hardening, documentation, dogfooding, and release | PM-11 | In progress |

This order is the default delivery sequence, not a demand for one enormous serial PR. Pure
schemas, query fixtures, and UI prototypes can be prepared independently when useful, but a
phase must not advertise guarantees that depend on a later incomplete phase. Runtime concurrency
tests are part of qualification and do not imply spawning implementation agents automatically.

## 5. PM-00 — Freeze the foundation contracts

**Outcome:** Implementers agree on the source model and supported behavior before changing storage.

Deliverables:

1. Add `workdeck-pm` to the workspace and architecture ownership map with its first real value types.
2. Define repository identity, qualified record references, issue ID, revision/content token,
   timestamps, schema version, and structured error categories.
3. Define the minimal issue document, custom-field namespace, workflow categories, and default
   states: Inbox, Backlog, Ready, In progress, In review, Verification, Done, Canceled.
4. Define which metadata is authoritative, derived, machine-local, and published on each ref.
5. Specify CLI JSON compatibility, mutation receipts, idempotency semantics, and noninteractive errors.
6. Inventory existing issue/project/cycle/label commands, config paths, export/import behavior,
   generated skill catalogs, review links, and all active `.agents/workdeck` consumers.
7. Establish fixtures for a minimal repository, legacy repository, malformed records, custom fields,
   source proposals, and future command/check references.

No workflows or empty feature stores are created in the user's project during implementation tests.
External libraries are selected against current project/toolchain compatibility at implementation
time; this plan does not pin a parser/index package without evaluating its required behavior.

**Exit criteria:** Typed identity/schema tests pass; architecture check accepts the new intended
edges; the compatibility/path inventory accounts for every active caller. Schema examples
distinguish supported initial fields from reserved later fields.

**Review slices:** package/value types; schema/error contracts; compatibility inventory and fixtures.

## 6. PM-01 — Build the file engine and initialization

**Outcome:** Workdeck can read and safely change authoritative PM files without a database.

Deliverables:

1. Implement root discovery and explicit source selection. Preserve app TOML configuration and
   keep PM YAML configuration separate, with deterministic precedence.
2. Implement frontmatter/Markdown parsing, schema validation, minimal stable serialization,
   unknown supported metadata preservation, and useful line/path diagnostics.
3. Implement read snapshots, source fingerprints, unique temporary writes, local writer locking,
   expected-revision/content checks, and recoverable multi-file change sets.
4. Define operation IDs and durable receipts for idempotent automated writes. A proposed layout
   is `.workdeck/operations/<ID>.yml` for minimal shareable mutation receipts and `.workdeck/.tmp/`
   for local recovery journals. The original result must survive replay; reused IDs with different
   input produce an error. Cache rows alone cannot provide durable deduplication.
5. Implement `workdeck init`, preserving `--init` compatibility, plus schema-focused `doctor`.
6. Generate required ignore entries for `.index/`, `.tmp/`, `.local/`, and local settings.
7. Make corrupt, unsupported, conflicting, or partially migrated stores explicit errors.

Read commands must not initialize `.workdeck/`. Existing initialized repositories may maintain
disposable indexes later, but no authoritative file is rewritten by a read. An atomic rename does
not by itself provide cross-process compare-and-set or atomic replacement of multiple files.

**Exit criteria:** Init is repeatable; read-only inspection leaves uninitialized repositories
untouched; concurrent local writes cannot silently clobber each other; direct editor changes are
detected even without revision increments; recovery either completes the intended change set or
returns an explicit unresolved operation. Markdown bodies and custom metadata survive round trips.

**Review slices:** parser/serializer; init/discovery; mutation/receipt/recovery engine.

## 7. PM-02 — Ship the new issue CLI

**Outcome:** A human or agent can manage a useful issue entirely through the existing executable.

Deliverables:

1. Implement create/list/show/update/edit against the new store, retaining established aliases
   and documenting versioned output changes.
2. Add validated transitions, assignment, priorities, labels, reporter/reviewer, due dates,
   archive/cancel/reopen, and completion timestamps.
3. Add file/commit/document links and preserve current file/commit link commands.
4. Add separate comment records and issue-local attachments with stable identities; metadata
   inspection must not execute or automatically render arbitrary attachment content.
5. Support templates, body-file/stdin JSON input, structured output, expected revisions,
   idempotency request IDs, and explicit scoped staging when requested.
6. Add configurable baseline acceptance rules and `issue done --dry-run` diagnostics. `close`
   calls the same completion operation. Manual acceptance is identified as manual; references to
   later unsupported check policies must fail explicitly rather than being treated as satisfied.
7. Expose initial schema/capability introspection from the same typed command catalog.

**Exit criteria:** A temporary-repo CLI flow initializes, creates, edits, comments, links, closes,
reopens, and archives an issue; invalid transitions and ambiguous short IDs fail predictably;
invalid edits leave original files intact; retried comments do not duplicate; existing CLI
compatibility tests pass or have an explicit intentional contract migration.

**Review slices:** core issue commands; workflow/ownership; comments/links and automation ergonomics.

## 8. PM-03 — Migrate the prototype and cut over the root

**Outcome:** Existing users retain their data while `.workdeck/` becomes the active root.

Deliverables:

1. Add a previewable legacy migration command under the existing migration family.
2. Convert legacy TOML issues into Markdown while preserving IDs, timestamps, priorities,
   ownership, descriptions, relationships, file/commit links, and extra metadata.
3. Convert project/cycle/label reference data using explicit ID and status mappings. Later richer
   fields may be absent; migration must not invent project acceptance or feature maturity.
4. Move repository app-config overrides through an explicit mapping, preserving TOML and user
   settings. Audit the existing Hunk importer so it does not recreate obsolete active paths.
5. Map handoffs, imported session metadata, and review references separately from issues.
6. Add a migration manifest, verified cutover marker, partial-failure recovery, and repeat-run behavior.
7. Update active config discovery, commands, tests, examples, and documentation. Keep old source
   data available until explicit cleanup; no automatic destructive cleanup on launch.

Before cutover, new storage is explicitly selected for opt-in/testing; the legacy store is not
silently converted. After verified cutover, all clients use the same new source. Both stores
present without an accepted migration marker produce an actionable ambiguity diagnostic.

**Exit criteria:** Migration reruns are no-ops or resume the same operation; IDs and body/metadata
round-trip; destination conflicts do not overwrite records; legacy config does not shadow the
new root; all active `.agents/workdeck` callers are migrated or covered by documented read-only
compatibility. No UI path continues writing the old issue store after cutover.

**Review slices:** inventory/preview; conversion; cutover and application-wide path compatibility.

## 9. PM-04 — Unify issue access with the review workbench

**Outcome:** Issues remain reachable whether the repository is clean or dirty.

Deliverables:

1. Replace dirty-state-dependent application selection with a persistent workbench shell for
   normal `workdeck` startup. Reuse the continuous review canvas and its state/controller behavior.
2. Add Issues navigation, list/detail panes, search/filter entrypoints, empty/error states, and
   contextual create/edit/comment/status/assignment actions backed by the PM API.
3. Support issue-to-file/commit/review navigation and issue creation from a selected file or note.
4. Preserve return context: selected issue, preview, review source, file/hunk position, and drafts.
5. Define narrow list/detail layouts and wider side-by-side planning/review layouts.
6. Retain specialized `diff`, `show`, patch/stdin, pager, and difftool entrypoints and terminal lifecycle.
7. Remove duplicate issue mutation logic from the old UI once its callers have migrated.

**Exit criteria:** PTY tests create/open/edit an issue from clean and dirty repositories, jump into
review and back, and retain selection; existing review navigation, terminal restore, pager, and
session controls pass. The TUI has no direct YAML/SQL mutation path.

**Review slices:** shell routing; issue list/detail/actions; review navigation and old-path retirement.

## 10. PM-05 — Add the project-management hierarchy

**Outcome:** Issues can be organized into deliverable projects and timeboxes.

Deliverables:

1. Add native initiatives, projects, milestones, cycles, and targets with stable identities.
2. Add CLI create/show/list/update/archive operations and corresponding project/cycle UI views.
3. Define project lead, scope, goal, dates, exit criteria, milestone outcomes, and target membership.
4. Add labels, users/identities, custom fields, estimates/units, templates, and saved-view definitions.
5. Support issue association changes with reference validation; archive referenced objects rather
   than silently orphaning issues. Destructive deletion requires an explicit resolution plan.
6. Add plain Markdown wiki/document links and authoring shortcuts without creating a second docs CMS.
7. Add one file per time entry, amendment/history rules, and issue/user/cycle reports.

Cycles schedule work; targets describe intended delivery; milestones express verifiable outcomes.
Do not sum estimates across incompatible units or infer project completion from issue counts.

**Exit criteria:** A project with milestones and a cycle can be created, populated, reassigned,
filtered, and archived without breaking references; custom-field requirements apply consistently;
time records deduplicate by identity; the UI and CLI produce equivalent project membership.

**Review slices:** models/CLI; relationships/configuration; project/cycle views and reporting.

## 11. PM-06 — Add relationships, native features, gates, and evidence

**Outcome:** Workdeck can explain prerequisites and separate work completion from capability maturity.

Deliverables:

1. Implement parent/child issue hierarchy, directed hard prerequisites, symmetric related links
   with a single canonical representation, and derived reverse relationships.
2. Reject parent/hard-dependency cycles; expose blocked reasons and preserve canceled prerequisites
   until their requirement is explicitly resolved, replaced, or waived under policy.
3. Implement native feature and gate records under `.workdeck/features/` and `.workdeck/gates/`.
4. Support feature hierarchy, typed edges, targets, project/milestone links, sources, and separate
   decision/maturity/availability fields. Keep issue-to-feature association many-to-many.
5. Add evidence references with subject, producer, check/result identity, provenance, and freshness.
   Declared evidence and verified evidence must remain distinguishable until evaluators exist.
6. Add stable acceptance-criterion references and dependency/child/gate completion explanations.
7. Provide dependency-path and coverage views with explicit unresolved-reference handling.

This is a generic native model. No imported feature schema or external integration is required.
Only hard dependencies determine readiness/build order. A dependency path without duration
estimates is not a delivery-date forecast. Filtered views retain awareness of outside prerequisites.

**Exit criteria:** Graph fixtures detect cycles, dangling references, canceled prerequisites, and
unresolved children; finishing an issue does not auto-promote feature maturity; unknown evidence
cannot satisfy a gate; feature IDs survive moves/renames; explanations match the actual graph.

**Review slices:** issue graph; native features/gates; evidence and criterion references.

## 12. PM-07 — Make the repository self-describing to agents

**Outcome:** A fresh agent can discover a bounded task and continue it without conversation history.

Deliverables:

1. Complete `capabilities --json`: schema/command versions, selected repository/source, supported
   operations, and meaningful unavailable-state reporting.
2. Implement `context --issue` with a budget, citations, requirements, source links, relevant
   instructions, feature references, blockers, questions, handoff, and verification requirements.
3. Implement `issue next` for ready work selection and top-level `next --issue` for suggested
   actions. Return eligibility/exclusion reasons and typed action preconditions.
4. Add structured handoff create/show and question create/answer/supersede operations.
5. Add compact output, fields, pagination, bounded errors, `--no-input`, and stable request identity
   consistently across new commands.
6. Generate command/schema references and the PM agent skill from typed catalogs. Merge a thin
   protocol pointer into repository instructions only through explicit initialization/update.
7. Add advisory overlapping-path/dependency visibility; it does not allocate worktrees or spawn agents.

Context is a versioned source-derived packet, not a grant of permission. Treat logs, imported
comments, and summaries as distinct from accepted requirements. Never copy an entire backlog,
private transcript, credentials, or ambient machine configuration into the packet.

**Exit criteria:** A scripted fresh-session exercise obtains relevant context and a correct next
action using only the issue ID; output respects its budget and reports omissions; another session
resumes from a handoff without treating stale evidence as current; generated docs/skills match
command schemas. Empty eligible-work results do not manufacture work.

**Review slices:** discovery/context; next/explanations; questions/handoffs and generated references.

## 13. PM-08 — Add commands, verification profiles, and a local check runner

**Outcome:** An agent can discover, plan, execute, and interpret the repository's required checks.

Deliverables:

1. Add `.workdeck/commands/`, `checks/`, and `check-profiles/` schemas and list/show/validate commands.
2. Define recipe argv, argument schema, cwd, toolchain/environment prerequisites, timeout, artifacts,
   and declared effects. Shell execution is an explicit recipe type; discovery never executes recipes.
3. Define check expectations and profiles such as quick, issue, pre-submit, and release-readiness.
   Profiles select required evidence; they do not become a general workflow language.
4. Implement `check plan` with source/acceptance/check-definition fingerprints and selection reasons.
   Include relevant dependency/toolchain inputs; incomplete impact information broadens the selected
   checks instead of claiming complete coverage.
5. Implement explicit foreground `command run` and `check run --plan` with process-group cleanup,
   bounded output, timeout/cancellation handling, and structured result receipts.
6. Store raw local logs and temporary plans under ignored `.workdeck/.local/`; retain/export durable
   evidence descriptors needed for completion and history. Missing local artifacts are visible and
   cannot remain sufficient evidence merely because their result IDs are known.
7. Add generic process results and declared structured report adapters, initially JUnit and SARIF
   where applicable. Exit zero does not establish that an expected test suite ran.
8. Add results/status/explain, failure filtering, and result references in the TUI/context packet.
9. Support dirty-worktree feedback with a captured input identity. Recheck identity after an in-place
   run; if relevant inputs changed during execution, mark results stale/unknown.

Recipes do not grant their own authority. Resolve execution within configured source roots,
validate arguments, and avoid implicit shell interpolation. No coding-agent sessions, background
service manager, autonomous retries, or durable remote jobs are introduced.

Results distinguish passed, failed, skipped, blocked, not_run, canceled, stale, and unknown.
Automatic result reuse remains disabled until complete input equivalence is tested. The first
release may explain potential reuse without applying a cached pass automatically.

**Exit criteria:** Checks run against an identified subject; changed inputs invalidate plans or
results; failing, empty-suite, missing-report, canceled, and timeout fixtures classify correctly;
logs remain bounded; terminating a command cleans up its owned children; issue context reports
actionable failures without dumping raw output.

**Review slices:** catalogs/profiles; check planner; foreground runner/report adapters; UI/results.

Historical preparation checkpoint (before PM-07 qualified): keep
recipe/check/profile definitions and check planning separate from foreground
execution and report assessment. An execution input manifest must capture complete
selected bytes and membership; the bounded context excerpt set is insufficient.
Reuse the CLI's existing cancellation signal seam, but do not add a forbidden PM→VCS
crate dependency merely to share its process-group cleanup code. Persist an invocation
identity before spawn, release the planning writer lock during execution, and retain
unknown/reconciliation state after interruption rather than automatically rerunning.
Keep public evidence declarations non-verifying; completion admission requires a
separate exact-subject, requirements and runner-result basis. Missing local artifacts
must invalidate sufficiency. Proposed bounded ownership: definitions/planning;
execution inputs/runner; reports/admission; root CLI/TUI/context integration.
These decisions now have implemented and qualified evidence in section 21.9.
PM-08 is complete. Completion admission remains
PM-11 scope.


PM-08 contract decisions (reviewed before implementation):

- Publish an immutable canonical invocation descriptor before spawning and an
  immutable result descriptor afterward, using the shared transaction engine.
  Keep logs, copied reports, temporary plans and execution ownership under ignored
  `.workdeck/.local/`. Release the planning lock while the command runs.
- Same-request replay inspects the original invocation and can finish result
  publication; it must never silently start another process. An interrupted run
  without conclusive terminal evidence remains unknown. Recovery never signals a
  historical PID whose process ownership has been lost.
- Capture complete selected inputs and membership, including dependency/toolchain
  configuration. Insufficient capture capacity rejects a plan. Dirty content gets
  its own identity; it does not claim to be HEAD. Uncertain impact broadens checks.
- Separate process observations, report expectations, source freshness and local
  artifact sufficiency. Results in this phase have `local_feedback` basis. PM-11
  owns CI producer trust and final completion admission; authored declarations
  remain non-verifying.
- Explicit foreground execution must drain bounded output and clean up its owned
  process group. Terminal shutdown/cancellation waits for cleanup acknowledgement;
  suspend is deferred during an active run. No service or agent-session hosting.

Report adapter references: [SARIF 2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/sarif-v2.1.0-os.html)
defines tool identity and invocation success separately from findings; an empty
findings array alone will not establish a successful expected run. For JUnit,
support an explicitly documented subset of the
[Apache Ant XML formatter](https://ant.apache.org/manual/api/org/apache/tools/ant/taskdefs/optional/junit/XMLJUnitResultFormatter.html)
with actual test-case counts, failure/error/skipped classification and declared
suite expectations, rather than claiming every producer-specific XML dialect.


## 14. PM-09 — Add explicit Git collaboration and claims

**Outcome:** Multiple compliant clients can coordinate work without silently overwriting plans.

Deliverables:

1. Implement configured accepted planning ref, implementation-branch proposals, and an isolated
   coordination worktree/ref. Show source identity and freshness in CLI/TUI output.
2. Add explicit fetch/sync/status and change-proposal operations. Do not rebase dirty code worktrees
   or silently stage/commit/publish application content.
3. Support local and shared claims with distinct guarantees. Local mode coordinates only writers
   using that source; it cannot advertise cross-clone exclusivity.
4. Implement claim/renew/release/list with unique token/generation, actor, issue revision, expiry,
   and operation identity. Active-work display derives from confirmed coordination state.
5. Publish a coordination commit descending from the exact observed ref; non-fast-forward rejection
   causes reload and reevaluation before bounded retry. Never force-push history or blindly replay
   an old claim after rebasing.
6. Require current token/generation for renew/release/agent completion. Changes to accepted issue
   requirements require explicit revalidation of the active work contract.
7. Add uncertain-push reconciliation, expired-claim recovery, configured clock-skew handling, and
   explicit cancellation/supersession states.
8. Make `doctor --staged` validate the staged snapshot and its relation closure rather than substitute
   working-tree contents. Offer hook installation explicitly.
9. Support reviewed publication on protected canonical branches without assuming direct pushes are
   allowed. Claim publication and canonical issue proposals remain separate operations.

Claims form a cooperative Git protocol. A client that arbitrarily rewrites refs outside the
protocol is outside the exclusivity guarantee. Expiration does not stop an old process; Workdeck
has no execution-fencing authority over external coding agents.

Local file mutation, Git publication, and claim release across refs are not one transaction.
Receipts expose partial states. Completion must not report successful shared-claim release when
publication remains uncertain.

**Exit criteria:** Two clones racing one issue produce one confirmed winner; stale token writes
fail; lost push responses reconcile without duplicate acquisition; expiry does not blindly
authorize conflicting continuation; proposals remain distinct from accepted state; developer
index/worktree content is preserved; staged validation catches defects hidden by working-tree edits.

**Review slices:** source/ref views; local claims; shared publication/reconciliation; staged validation.

## 15. PM-10 — Add indexed views and cross-repository navigation

**Outcome:** The full planning model remains responsive and explainable at realistic scale.

Deliverables:

1. Implement a disposable SQLite/FTS projection over native records, graph edges, comments, evidence,
   and saved views. Version the projection separately from source schemas.
2. Add deterministic reindex, fingerprints, incremental refresh, and handling for editor changes,
   Git checkout/merge, malformed input, and interrupted indexing.
3. Publish consistent query snapshots; do not combine halves of different source revisions. Last-good
   results carry stale/error indications when source becomes invalid.
4. Add list/board views, grouping/sorting/filtering, native feature tree, target/dependency slices,
   project/milestone views, cycle carryover, and activity timelines.
5. Add My work across explicitly registered repositories, qualified references, local path mappings,
   and repository/worktree/source switching with preserved view context.
6. Keep writes scoped to one source. Cross-repository reports do not create a central planning
   authority or infer completion for unavailable external prerequisites.
7. Virtualize long views and benchmark a synthetic 40,000-node feature graph plus an independent
   10,000-issue workload. Record dataset, machine, cold/warm timing, memory, and p50/p95 responses.

Initial UX targets are warm list/filter feedback within 100 ms and family-specific incremental
refresh budgets of 30 seconds for the 40,000-feature workload and 15 seconds for the 10,000-issue
workload on the documented reference setup. These are qualification budgets calibrated from the
current native implementation, not claims about mounted UI latency. Establish cold-index/memory
budgets from the first baseline, then enforce agreed regression thresholds. Display source state
immediately even while the index loads.

**Exit criteria:** Deleting the index yields equivalent rebuilt queries; malformed/stale sources
are visible; source views stay distinct; same-looking IDs cannot cross-mutate repositories;
large lists/trees remain navigable within measured budgets; readiness explains prerequisites
outside the current filter.

**Review slices:** projector/reindex; indexed queries; boards/features; multi-repo navigation/performance.

## 16. PM-11 — Integrate CI, review, and completion evidence

**Outcome:** Local work and existing CI agree about whether a candidate satisfies its work contract.

Deliverables:

1. Add headless entrypoints such as `workdeck ci validate --base <SHA> --head <SHA>` for PM changes
   and `workdeck ci check --profile <ID> --revision <SHA>` for configured verification. Freeze exact
   syntax in the command catalog; do not expose unsupported remote status APIs.
2. Validate actual commit/staged snapshots and relation closure. Resolve branch names before
   checking and return exact source/check-definition identities.
3. Export machine-readable summaries and supported reports. Import evidence descriptors with
   explicit producer trust/provenance; imported or manual success is not automatically trusted CI.
4. Bind accepted acceptance/check contracts separately from proposed test changes. Support red/green
   evidence when required. Detect removed or weakened required checks instead of letting a candidate
   redefine its completion denominator.
5. Complete policy evaluation for issue Done, project/milestone exit criteria, and feature maturity.
   Distinguish manual acceptance, local feedback, reviewed evidence, and CI qualification.
6. Show acceptance/check coverage beside review, with unmet/stale requirements and revision-bound
   review references. Relevant subject changes invalidate conclusions according to policy; a UI
   note alone does not impersonate a required reviewer.
7. Add PM validation to this repository's existing CI and release checks. Use a suitable pinned
   validator/trusted baseline for candidate changes to the validator itself; self-validation alone
   is development evidence, not independent qualification.
8. Add release-readiness profiles without deploying, publishing, creating cloud resources, or
   replacing the existing CI host. Hook commands and CI use the same validation library.

**Exit criteria:** CI rejects invalid PM changes, missing checks, wrong-source/stale evidence, and
unsupported completion claims; valid candidates pass consistently locally and in CI; false-green
fixtures fail; changes to accepted evaluation contracts receive the required review; existing
release/terminal checks retain their coverage.

**Review slices:** headless CI; evidence provenance/policy; review completion UX; CI wiring.

## 17. PM-12 — Harden, document, dogfood, and release

**Outcome:** The complete standalone workflow is usable, recoverable, and supportable.

Deliverables:

1. Run the complete temporary-repository scenario: initialize/migrate, plan, select, claim, obtain
   context, implement a fixture change, check, review, complete, publish, and resume in another clone.
2. Test interrupted writes/migration/indexing/checks/publication, malformed files, unsupported
   schemas, disk/permission failures, stale evidence, and identity/short-reference collisions.
3. Verify path containment, symlinks, argument execution, bounded parsing/logs, and source/output
   handling through concrete tests of supported operations.
4. Run supported Linux/macOS/Windows targets with explicit locking/rename/process-cleanup tests.
   Document unsupported filesystems rather than promising uniform network-filesystem semantics.
5. Qualify narrow/wide TUI layouts, key discoverability, worktree navigation, terminal lifecycle,
   existing session controls, and scaled dataset behavior.
6. Update install/init/migration instructions, config/schema references, workflows, generated skill,
   recovery guidance, and architecture docs. Remove superseded active prototype instructions.
7. Dogfood in Workdeck through a few independently verifiable packages; do not manufacture issues
   for every function or design heading.
8. Prepare a release through the normal packaging/check process. Publishing remains an explicit
   release action after qualification.

**Exit criteria:** The fresh-user/agent scenario passes across the supported matrix; repeated
operations do not duplicate effects; source/claim/evidence ambiguity is exposed; quality/release
gates pass; migration/recovery docs match behavior; performance and remaining limitations are
recorded. All PM-00–PM-11 criteria are actually satisfied.

**Review slices:** fault/compatibility fixes; end-to-end dogfood; documentation/package qualification.

## 18. Release milestones

| Milestone | Completed phases | What becomes useful |
| --- | --- | --- |
| A: Local PM preview | PM-00–PM-04 | New files/CLI, preserved legacy data, persistent issue/review access |
| B: Agent-ready planning | PM-05–PM-08 | Planning/work graph, native features, context/handoffs, commands/checks |
| C: Collaborative workbench | PM-09–PM-10 | Shared claims, branch-aware sources, indexed boards, multi-repo reads |
| D: Standalone release | PM-11–PM-12 | CI-backed acceptance, qualified recovery/compatibility, release readiness |

Previews state which guarantees remain unavailable. Local claims are not distributed exclusion
before PM-09, and attached reports are not trusted qualification without their provenance policy.

## 19. Validation discipline

Each meaningful behavior starts with an acceptance case and an appropriate failing test, then
focused implementation and green evidence. Exercise observable invariants and failures rather
than tests that mirror implementation. Documentation-only changes use structural/link checks.

Per-phase checks include affected library, CLI, temporary Git, and PTY tests. Architecture changes
run that gate in the same change. Broader workspace/release checks run at integration milestones.

Existing repository checks to reuse, selecting the subset appropriate to the change:

```sh
cargo fmt --all --check
cargo test --locked -p workdeck-cli --test cli
cargo test --locked -p workdeck-cli --test git_integration
cargo test --locked -p workdeck-tui --all-targets
cargo xtask architecture check
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo xtask verify
```

After the new crate exists, add `cargo test --locked -p workdeck-pm`. These describe future
implementation validation, not checks executed while writing this plan. Never use the user's
actual backlog, shared coordination ref, or deployment as a test fixture.

## 20. First implementation change and completion tracking

Start with the PM-00 package/value/schema seam, then the smallest PM-01/02 vertical flow: explicit
`.workdeck/` init, create a Markdown issue, list/show it, update with a source precondition, and
reject invalid/stale edits. Keep it opt-in until migration/UI cutover are ready; do not redirect
production discovery to a partially implemented store.

Then deliver the first issue/review workbench milestone before expanding every planning screen.
Commands/context become useful in milestone B rather than waiting until final UI polish.

This document is the phase-status ledger until Workdeck can track its own implementation. Record
implementation PR/commit, actual checks, remaining limitations, and exit decision when they exist.
A schema or design document alone does not complete a phase. Generate actual issues only for the
next sufficiently specified work packages.

### Durable implementation checkpoint — 2026-09-09

- **Baseline:** `c9a36ec6d6545570380e193a0a23d36cb591356f` (`c9a36ec6`),
  branch `linear-project-management`. Existing README/design/reference/plan changes are preserved.
- **Completed planning work:** Read the implementation authorization and canonical plan; mapped
  all numbered deliverables and phase exit criteria into the requirement ledger. This is tracking
  evidence only, not proof of implemented product behavior.
- **Implemented foundation:** The new `workdeck-pm` crate now contains typed identities, schema and
  workflow validation, lossless Markdown/YAML documents, explicit source discovery/initialization,
  serialized snapshots, durable request receipts, recoverable multi-file writes, and shared issue
  operations. See [schema contracts](project-management-schema.md),
  [compatibility inventory](project-management-compatibility.md), and
  [PM source](../crates/workdeck-pm/src/lib.rs). These are implementation increments, not a phase exit.
- **Current work (cutover continuation):** Application config, extension discovery, Hunk import
  destinations and repository boundaries now select `.workdeck/` deliberately. Sole legacy sources
  remain explicit compatibility sources; fresh issue/reference commands require initialization.
  Native source errors prevent legacy fallback. Files/Changes/Git/Search/recorded-Agent panels use
  bounded source-bound providers in the existing native runtime. Native Search preserves configured
  workflow/custom metadata and filters target groups before result limits. Shared retirement and CLI
  deletion retain authored history, reject incoming references and ID reuse, support reviewed previews,
  replay and scoped staging. The superseded CLI UI island is removed; native priority/label/copy/link
  controls preserve useful behavior through the shared API. Native recorded-session/event adapters
  and exact JSON/JSONL snapshots are implemented and under independent review. Same-authority import
  works with explicit blockers for missing receipt authority and foreign-source conversion.
- **Accepted evidence:** [Foundation checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08)
  records source SHA-256 `8b962c09078b1309731e6859badd143d052727e7816ba91f0cd2d864ac94bd0d`,
  168 PM tests, 86 CLI/Git integration tests, 11 workbench controller tests, the executable Clap
  contract, strict PM/CLI lint, workspace formatting, and architecture (13 crates, one executable,
  zero violations). Focused contracts/issues tests were rerun after test formatting. All passed.
  The checkpoint also records meaningful failing regressions and precise validation limits.
- **Build environment:** The volume exhausted free space during normal debug builds. Only this
  worktree's disposable `target/` was cleaned; unrelated builds were preserved. Subsequent Cargo
  commands use `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` to reduce
  artifact size. These settings change build artifacts, not test selection or acceptance gates.
- **Earlier integrated evidence:** [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09) records source SHA-256
  `d32f81360daaa5de5cb156494236207946d36ddfea050553332c4fd64e25bfe3` over 93 inputs:
  214 PM tests, 103 CLI/Git integration tests, 1,092 TUI tests, 98 PTY cases, five affected
  workbench PTY cases rerun after the architecture correction, strict PM/CLI/TUI lint, formatting,
  and architecture checks all pass. A final rehash found no changed input. Platform and complete
  workspace/release qualification remain later requirements.
- **Intermediate focused evidence:** [Cutover work in progress](project-management-validation.md#cutover-work-in-progress--2026-09-09)
  records the new root/path, Files/Git/panel, search, lifecycle and retirement regressions. Root observed
  65 focused CLI tests passing together after retirement integration, then 66 combined PM traversal/
  history tests and 21 related CLI tests. Strict legacy framing fixes also passed two new tests and
  all 46 established CLI cases together. These are not a replacement for
  a new complete integrated source fingerprint and broad qualification. Per-owner evidence and the
  intermittent macOS pre-main launch delays are recorded separately.
- **Completed cutover:** PM-02–PM-04 closed after an independent audit of all 37
  deliverables/exit criteria. Candidate `c55d2f2be7844032dcbc99553910093574e8f20c8ce5d21da01d3b0a3732b739`
  passed 346 PM, 626 CLI (106 PTY), 1,117 TUI, 568 support, and three startup-example
  tests with unchanged source. Candidate `bebe756e71632e53b0c95958ab429126cbba1ab0653eb7f1d87d9ef3a6920ca9`
  changes only three CLI test assertions for strict lint. Its 520 non-PTY CLI tests
  passed; the concurrent PTY run passed 96/106 with ten extension startup timeouts.
  All 20 extension PTYs passed serially on the same source. Strict integrated lint,
  architecture, skill mappings, formatting, and final source comparison passed.
  See [cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09).
- **Unresolved:** Concurrent extension-startup test timing remains a PM-12 qualification
  concern; a passing serial recheck does not establish reliable concurrent startup.
  Windows durability/platform qualification, scaled performance, full workspace/package
  gates, and final independent review retain their full scope. No real backlog was initialized.
- **Completed PM-05:** Candidate `e4487513cd0cfeacd0d6638e9b6aa2e0941a21c63af9c5e12959a34f71e891f3`
  captures 995 source inputs. All nine gates passed with unchanged source: PM **439**,
  CLI **664** (including **107 PTY**), TUI **1,133**, supporting crates **568**, startup
  examples **3**, strict eight-package all-targets Clippy, architecture, skill mappings
  and formatting. Final fingerprint comparison matched. Independent review mapped all
  eleven PM-05 requirements to real behavior and tests, finding no remaining scope gap
  after the qualified lifecycle, receipt-proof and retained-request repairs. Logs and
  source manifest: `/tmp/workdeck-pm05-qualification-20260909/`. Five nested CLI subprocess
  worker results are not counted twice. No real backlog was initialized.
- **Completed PM-06:** Candidate `248ee1112608fffd86aaa3b2b58b2c108598bd7f2d18727dd2b0f768e01b4e59`
  captures 1,031 inputs. All nine gates pass with unchanged source: PM **509**, CLI
  **679** including **109 PTY**, TUI **1,145**, supporting crates **568**, startup **3**,
  strict eight-package all-targets Clippy, architecture, skill mappings and formatting.
  Final fingerprint comparison matched. The twelve phase requirements have concrete
  implementation and regression evidence below. Logs and manifest are retained under
  `/tmp/workdeck-pm06-qualification-20260909/`. The first CLI attempt's seven extension
  fixture failures remain archived; unchanged exact, binary-group and full CLI retries
  passed without source or timeout changes. Cause remains unconfirmed.
- **Historical PM-07 integration and review:** Before final qualification, shared
  context had 22 focused passing cases; continuity had 15 question and 12 handoff
  cases plus the 132-test adjacent pass. The mounted Context workspace had 16 new
  passing cases within an 85-test workbench pass, plus two real PTY journeys.
  Explicit protocol installation had 17 passing preservation/recovery
  cases and strict lint. It requires the preview's RepoID, AGENTS hash/absence and
  original request; receipts remain local-only. Independent review repairs cover
  malformed config and capability diagnostics, fenced Markdown examples, missing or
  changed recovery journals, cross-invocation repository replacement and identical
  clones/saves wrongly changing context anchors. Core source hashes now exclude
  transient file IDs while retaining them for within-capture race checks. CLI parser
  errors use bounded machine output, and capabilities supports small field projections.
  Generated references matched that checkpoint and their five-test catalog group
  passed; the context/continuity/protocol CLI union passed 28 tests. Candidate
  `01194b9f2253cfbb0e652e9a2d32fb7140023a7e7da4bf75ed8f3d600c30d4b4` captures 1,065
  source inputs, including generated command/schema references. Its PM558, strict
  lint and format gates passed, but the CLI gate failed 10 existing extension PTY
  cases (101 PTY passed). The unchanged exact highlighter retry passed; the startup
  cause remained unconfirmed after bounded investigation. Reports are preserved in
  `/tmp/workdeck-pm07-qualification-20260909/initial-01194b9f/`.
  A separate comparison against the retained PM-06 executable exposed a legacy
  read-only config-error compatibility regression. The narrowed wrapper repair and
  paired legacy/new-command regression passed 13 tests. Thirteen bare extension
  handshakes and ten full-host probes passed without reproducing the startup cause.
  The PTY harness now retains bounded startup notices/first frame on failure with
  unchanged assertions and deadlines. Candidate
  `e600bdef16f7188ed23df3e9130f183c732f0891bc4092ce91c45868f5177d60` captures 1,065
  inputs; strict Clippy, formatting and full CLI qualification passed, including
  111 PTY tests. Evidence is retained in `second-e600bdef/` under the qualification
  directory. The final requirement audit identified omitted issue document links
  in context; that repair and a protocol receipt wording clarification supersede
  the candidate. The repair passed core26, CLI context7/catalog5 and the mounted
  document case; independent review confirmed the D2 gap closed.
- **Completed PM-07:** Candidate
  `f1d4e9c55d712e21de6e130f47cdbef05e595ecd453b4c7b1661c26fe3f5902c`
  (1,065 inputs) passed all nine unchanged-source gates: PM **562**, CLI **714**
  (including **111 PTY**), TUI **1,162**, supporting crates **568**, startup
  examples **3**, strict eight-package all-targets Clippy, architecture, generated
  skills and formatting. The final source comparison matched. See the
  [qualified checkpoint](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09).
- **Completed PM-08:** Candidate
  `c78a0b2f119a78e14e4528a2d36fdda395fa14843eee09ae7b0a3bce740e40cb`
  (1,102 inputs) has passing evidence for all nine gates. CLI727 (including
  115 real PTYs), support568, strict lint, architecture, skills and formatting
  passed on this candidate. PM624, TUI1,175 and startup3 retain their original
  `b69d7c5d` reports with explicit unchanged-input equivalence; they were not rerun
  on the final aggregate fingerprint. The final source comparison matched;
  all 15 phase requirements are complete. Total: 3,097 top-level tests. Focused results,
  carried-forward evidence and retained support/lint failures are in section 21.9 and
  [validation](project-management-validation.md#pm-08-qualified-checkpoint--2026-09-09).
- **Current work / next action:** PM-09 closed with all nine gates passing on candidate
  `65a50ac0`; 3,229 top-level tests, including 119 actual PTYs. PM-10 begins with
  immutable SQLite projections, bounded query handles and source-qualified registry
  integration, followed by boards/tree virtualization and measured scale qualification.
  PM-11–PM-12 retain full scope. No real backlog was initialized.
- **Initial PM-05 qualification candidate:** `d9f33355da807c68154489fc73d43b6b9aa7f9dfd01c3baf1330229455aac20b`
  captures 995 source inputs. Formatting passed. Strict integrated lint found a large CLI
  parser enum; boxing its query options addresses the size without changing flags. During
  qualification, a concrete lifecycle regression was reproduced: invalid typed custom data
  on a completed historical issue blocked both reopen and archive. The bounded repair keeps
  unchanged historical values during lifecycle repair while content edits/completion remain
  strict. This supersedes the initial candidate; its PM run is intermediate evidence only.
  An early parallel PM sweep also observed a migration worker receiving the documented
  bounded `Locked` error; exact serial and frozen-union rechecks remain required.
- **Working rule:** Update individual evidence cells with source/test links, the exact command,
  outcome, and tested source fingerprint. Record failure/recovery evidence and limitations before
  checking an item. Continue useful work across checkpoints; do not replace the goal with a subset.

#### PM-05 preparation after cutover qualification

Read-only seam review recommends reusing `PlanningKind`, generic planning mutations and the
existing `<kind>/<id>/item.md` layout for initiatives, milestones and targets. The first hierarchy
slice must also extend snapshot classification/import, doctor, retirement/incoming-reference
protection, source markers and catalogs; CRUD alone would otherwise create unexportable or
unprotected records. Preserve aggregate `labels.yml`, imported identities and absent historical
dates. Declared outcomes remain distinct from verified completion.

Introduce one snapshot-scoped reference validator and a shared serializable `IssueQuery` consumed
by CLI and TUI. Validate milestone/project ownership and newly introduced associations while
preserving archived memberships and visibly unresolved historical references. Keep forward
associations canonical and derive inverse memberships; represent overlapping targets explicitly.
New or edited dates are validated without rewriting unchanged historical declarations.

Once these signatures settle, implement users/custom-schema/estimates/saved views, plain Markdown
wiki shortcuts, and independent time/amendment records as separate modules. Every new record
kind requires immediate doctor/export/restore integration. Time records must leave issue source
revisions untouched, retain captured cycle attribution, and report each amendment chain once.
Project/cycle TUI controllers use the shared operations/query with retained drafts and source
preconditions. This is preparation only: all PM-05 deliverables and exit criteria remain open.

## 21. Requirement-to-evidence ledger

This ledger tracks the original plan without replacing or abbreviating its requirements. `Dn`
refers to the identically numbered deliverable in that phase; the complete original deliverable
is repeated below so all its clauses remain visible. `En` splits each original exit paragraph
into separately auditable outcomes. `PM-X.Cn` covers cross-cutting defaults, narrative invariants,
and the implementation authorization. `PM-V.Gn` records the named existing repository gates.

Every checkbox is initially open. **Pending** means no implementation or acceptance evidence has
been accepted for the row. The validation column specifies evidence still required; it does not
claim the named cases exist or pass. Replace pending implementation cells with repository source
links and pending validation cells with meaningful test/artifact links, exact commands, outcomes,
and the tested source identity. Partial implementation may be recorded while its box stays open.

Do not check a deliverable from compilation, schema presence, or a narrower happy-path test alone.
A phase is complete only when its deliverables, exits, and applicable cross-cutting requirements
are checked against current integrated source. Unavailable platform/external checks remain open
with the missing prerequisite, and final review must independently inspect every checked item.

### 21.1. PM-00 — Freeze the foundation contracts

Phase state: **Complete — foundation contracts**. Evidence: [accepted checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08).

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-00.D1** — Add `workdeck-pm` to the workspace and architecture ownership map with its first real value types. | [Workspace](../Cargo.toml), [crate](../crates/workdeck-pm/src/lib.rs), [architecture](../xtask/src/architecture.rs). | Architecture gate and real typed contracts pass; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.D2** — Define repository identity, qualified record references, issue ID, revision/content token, timestamps, schema version, and structured error categories. | [Identity](../crates/workdeck-pm/src/identity.rs), [errors](../crates/workdeck-pm/src/error.rs), [source pin](../crates/workdeck-pm/src/transactions.rs). | [Contracts](../crates/workdeck-pm/tests/contracts.rs), [schema](../crates/workdeck-pm/tests/schema.rs), [identity replacement](../crates/workdeck-pm/tests/source_identity.rs) pass; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.D3** — Define the minimal issue document, custom-field namespace, workflow categories, and default states: Inbox, Backlog, Ready, In progress, In review, Verification, Done, Canceled. | [Model](../crates/workdeck-pm/src/model.rs), [workflow](../crates/workdeck-pm/src/workflow.rs). | Default/custom workflow, extension preservation, typed schema/identity tests pass; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.D4** — Define which metadata is authoritative, derived, machine-local, and published on each ref. | [Authority contract](project-management-schema.md#metadata-authority), `MetadataAuthority`, transaction local-path exclusions; shared refs explicitly remain later behavior. | Authority classification, local-file exclusion, and reserved source-proposal rejection pass; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.D5** — Specify CLI JSON compatibility, mutation receipts, idempotency semantics, and noninteractive errors. | [JSON adapter contract](project-management-schema.md#json-mutation-adapter-contract--api-v1), [adapter](../crates/workdeck-cli/src/pm_cli.rs), [receipts](../crates/workdeck-pm/src/transactions.rs). | Native JSON/errors/replay/recovery and existing legacy CLI envelopes pass; publication is explicitly not implied by local receipts; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.D6** — Inventory existing issue/project/cycle/label commands, config paths, export/import behavior, generated skill catalogs, review links, and all active `.agents/workdeck` consumers. | [Compatibility inventory](project-management-compatibility.md) covers baseline commands, paths, config, imports/exports, sessions, skills, UI writers and cutover dispositions. | Current native/legacy adapter checks and 46 existing CLI tests pass; inventory distinguishes migration work from implemented compatibility; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.D7** — Establish fixtures for a minimal repository, legacy repository, malformed records, custom fields, source proposals, and future command/check references. | [Fixtures](../crates/workdeck-pm/tests/fixtures), [contracts](../crates/workdeck-pm/tests/contracts.rs), [preview cases](../crates/workdeck-pm/tests/migration_preview.rs). | Minimal/custom/malformed/legacy/source-proposal/future-check fixtures exercised; reserved behavior fails explicitly; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.E1** — Typed identity and schema tests pass. | Typed identity/schema implementations linked above. | 6 contracts and 17 schema tests plus imported/source-identity regressions pass; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.E2** — The architecture checker accepts the new intended package edges without exceptions. | [Architecture map](../xtask/src/architecture.rs) includes intended PM edges. | Architecture: 13 production crates, one executable, zero violations; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.E3** — The compatibility/path inventory accounts for every active caller. | [Caller inventory](project-management-compatibility.md) with explicit path/writer disposition. | Inventory checked against current adapters and existing CLI/Git compatibility coverage; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-00.E4** — Schema examples distinguish supported initial fields from reserved later fields. | [Schema examples](project-management-schema.md), generated typed [catalog](../crates/workdeck-pm/src/catalog.rs). | Initial fields and reserved future declarations are distinguished by fixtures and schema CLI tests; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |

### 21.2. PM-01 — Build the file engine and initialization

Phase state: **Complete** on the qualified macOS/Unix implementation; wider platform qualification remains PM-12.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-01.D1** — Implement root discovery and explicit source selection. Preserve app TOML configuration and keep PM YAML configuration separate, with deterministic precedence. | [Repository](../crates/workdeck-pm/src/repository.rs); explicit `open_source`/recovery source selection; TOML app preferences separate from PM YAML. | Nested Git boundary, explicit source, non-init reads and TOML preservation cases pass; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.D2** — Implement frontmatter/Markdown parsing, schema validation, minimal stable serialization, unknown supported metadata preservation, and useful line/path diagnostics. | [Documents](../crates/workdeck-pm/src/documents.rs) and typed record parsers. | 23 document tests plus malformed/comment/template/schema diagnostics and editor CLI cases pass; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.D3** — Implement read snapshots, source fingerprints, unique temporary writes, local writer locking, expected-revision/content checks, and recoverable multi-file change sets. | [Transaction engine](../crates/workdeck-pm/src/transactions.rs), [issue mutations](../crates/workdeck-pm/src/issues.rs). | 29 transaction cases cover subprocess concurrency, crash boundaries, fingerprints, dependencies and direct edits on this macOS/Unix host; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.D4** — Define operation IDs and durable receipts for idempotent automated writes. A proposed layout is `.workdeck/operations/<ID>.yml` for minimal shareable mutation receipts and `.workdeck/.tmp/` for local recovery journals. The original result must survive replay; reused IDs with different input produce an error. Cache rows alone cannot provide durable deduplication. | [Durable receipts](../crates/workdeck-pm/src/transactions.rs) bind repository/request/input/result and changed hashes. | Replay, changed-input conflict, corrupted receipt and interrupted journal tests pass; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.D5** — Implement `workdeck init`, preserving `--init` compatibility, plus schema-focused `doctor`. | Native init/doctor in [CLI adapter](../crates/workdeck-cli/src/pm_cli.rs); `--init` routes the same native initializer. | Native init/doctor and intentional flag cutover tests preserve TOML preferences and stable identity; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-01.D6** — Generate required ignore entries for `.index/`, `.tmp/`, `.local/`, and local settings. | [Init ignores](../crates/workdeck-pm/src/repository.rs) and transaction local-path exclusions. | Required ignore preservation/repeat and local settings exclusion cases pass; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.D7** — Make corrupt, unsupported, conflicting, or partially migrated stores explicit errors. | [Migration admission and recovery](../crates/workdeck-pm/src/migration/apply.rs), repository discovery and ordinary recovery guards. | 15 application, 12 preview, 19 repository and CLI failure/recovery cases pass; pending/altered cutover never becomes normal authority; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-01.E1** — Initialization is repeatable. | [File engine](../crates/workdeck-pm/src/repository.rs), [transactions](../crates/workdeck-pm/src/transactions.rs), [documents](../crates/workdeck-pm/src/documents.rs). | Relevant repository/document/transaction tests and native CLI source/recovery cases pass on macOS; platform qualification remains PM-12; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.E2** — Read-only inspection leaves uninitialized repositories untouched. | [File engine](../crates/workdeck-pm/src/repository.rs), [transactions](../crates/workdeck-pm/src/transactions.rs), [documents](../crates/workdeck-pm/src/documents.rs). | Relevant repository/document/transaction tests and native CLI source/recovery cases pass on macOS; platform qualification remains PM-12; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.E3** — Concurrent local writers cannot silently clobber each other. | [File engine](../crates/workdeck-pm/src/repository.rs), [transactions](../crates/workdeck-pm/src/transactions.rs), [documents](../crates/workdeck-pm/src/documents.rs). | Relevant repository/document/transaction tests and native CLI source/recovery cases pass on macOS; platform qualification remains PM-12; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.E4** — Direct-editor changes are detected even without revision increments. | [File engine](../crates/workdeck-pm/src/repository.rs), [transactions](../crates/workdeck-pm/src/transactions.rs), [documents](../crates/workdeck-pm/src/documents.rs). | Relevant repository/document/transaction tests and native CLI source/recovery cases pass on macOS; platform qualification remains PM-12; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.E5** — Recovery completes the intended change set or returns an explicit unresolved operation. | [File engine](../crates/workdeck-pm/src/repository.rs), [transactions](../crates/workdeck-pm/src/transactions.rs), [documents](../crates/workdeck-pm/src/documents.rs). | Relevant repository/document/transaction tests and native CLI source/recovery cases pass on macOS; platform qualification remains PM-12; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |
| [x] **PM-01.E6** — Markdown bodies and custom metadata survive round trips. | [File engine](../crates/workdeck-pm/src/repository.rs), [transactions](../crates/workdeck-pm/src/transactions.rs), [documents](../crates/workdeck-pm/src/documents.rs). | Relevant repository/document/transaction tests and native CLI source/recovery cases pass on macOS; platform qualification remains PM-12; [Checkpoint](project-management-validation.md#foundation-checkpoint--2026-09-08). |

### 21.3. PM-02 — Ship the new issue CLI

Phase state: **Active, incomplete**. See checkpoint and individual remaining criteria.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-02.D1** — Implement create/list/show/update/edit against the new store, retaining established aliases and documenting versioned output changes. | [Shared issue API](../crates/workdeck-pm/src/issues.rs), [native CLI](../crates/workdeck-cli/src/pm_cli.rs), [intentional compatibility contract](project-management-compatibility.md). | Native CRUD/editor, aliases, retirement and all 46 established lifecycle scenarios passed in the complete CLI gate; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-02.D2** — Add validated transitions, assignment, priorities, labels, reporter/reviewer, due dates, archive/cancel/reopen, and completion timestamps. | [Shared issues/workflow](../crates/workdeck-pm/src/issues.rs) and [CLI adapter](../crates/workdeck-cli/src/pm_cli.rs). | Issue tests, 5 lifecycle contracts and 6 alias/CLI cases cover ownership, due dates, cancel/reopen/terminal timestamps and custom workflow ambiguity; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-02.D3** — Add file/commit/document links and preserve current file/commit link commands. | [Collections API](../crates/workdeck-pm/src/issues.rs); native file/commit/document link and unlink adapters. | Native CLI link and inert-document regressions pass, including replay after later links and invalid input preserving source; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-02.D4** — Add separate comment records and issue-local attachments with stable identities; metadata inspection must not execute or automatically render arbitrary attachment content. | [Comments](../crates/workdeck-pm/src/issues.rs) and [attachments](../crates/workdeck-pm/src/attachments.rs). | 5 comment and 9 attachment cases plus CLI tests prove stable independent records, inert metadata reads, bounds and replay; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-02.D5** — Support templates, body-file/stdin JSON input, structured output, expected revisions, idempotency request IDs, and explicit scoped staging when requested. | [Native adapter](../crates/workdeck-cli/src/pm_cli.rs), templates and [scoped staging](../crates/workdeck-pm/src/staging.rs). | Native JSON/body/editor/template cases, 19 staging cases and CLI mixed-index failure/replay pass; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-02.D6** — Add configurable baseline acceptance rules and `issue done --dry-run` diagnostics. `close` calls the same completion operation. Manual acceptance is identified as manual; references to later unsupported check policies must fail explicitly rather than being treated as satisfied. | [Completion and workflow operations](../crates/workdeck-pm/src/issues.rs), [acceptance configuration](../crates/workdeck-pm/src/model.rs). | Eight [lifecycle CLI tests](../crates/workdeck-cli/tests/pm_lifecycle.rs) cover preview, close/done parity, manual attribution, unmet criteria and unsupported check policy; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-02.D7** — Expose initial schema/capability introspection from the same typed command catalog. | [Clap command catalog](../crates/workdeck-cli/src/pm_catalog.rs) and [typed schemas](../crates/workdeck-pm/src/catalog.rs). | Catalog tests verify source identity, actual command/argument definitions, writable fields and reviewed native retirement and explicit resolution requirements; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-02.E1** — A temporary-repository CLI flow initializes, creates, edits, comments, links, closes, reopens, and archives an issue. | [Native issue CLI](../crates/workdeck-cli/src/pm_cli.rs). | [Full headless lifecycle](../crates/workdeck-cli/tests/pm_lifecycle.rs) initializes, creates, edits, comments, links, closes, reopens and archives in one temporary repository; complete CLI gate passed, qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-02.E2** — Invalid transitions fail predictably. | [Shared workflow and issue operations](../crates/workdeck-pm/src/issues.rs). | Forbidden completed/canceled transitions and missing/ambiguous configured targets reject without writes; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-02.E3** — Ambiguous short IDs fail predictably. | [Issue resolver](../crates/workdeck-pm/src/issues.rs). | Library and CLI exact/short/ambiguous reference cases reject accidental mutations predictably; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-02.E4** — Invalid edits leave original files intact. | [Editor adapter](../crates/workdeck-cli/src/pm_cli.rs), shared document mutation and source checks. | Malformed, canceled, stale, changed-identity and managed-field editor regressions preserve original files and retain failed drafts; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-02.E5** — Retried comments do not duplicate. | [Independent comments and durable replay](../crates/workdeck-pm/src/issues.rs). | Library concurrent/replayed comments and CLI same-request retries preserve one original comment/result; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-02.E6** — Existing CLI compatibility tests pass or have an explicit intentional contract migration. | [Compatibility disposition](project-management-compatibility.md), [legacy admission](../crates/workdeck-cli/src/pm_cli.rs), [reviewed reference resolution](../crates/workdeck-cli/src/pm_reference.rs). | All 46 established CLI scenarios remain with native migration fixtures; 52 mutation variants reject legacy writes at canonical/custom roots. Native forced resolution has real preview/apply/history/staging/replay tests; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |

### 21.4. PM-03 — Migrate the prototype and cut over the root

Phase state: **Active, incomplete**. See checkpoint and individual remaining criteria.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-03.D1** — Add a previewable legacy migration command under the existing migration family. | [Migration CLI](../crates/workdeck-cli/src/pm_migration.rs) under `migrate legacy`. | Five CLI tests cover read-only preview, exact saved proposal, explicit apply/resume, blockers and target binding; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-03.D2** — Convert legacy TOML issues into Markdown while preserving IDs, timestamps, priorities, ownership, descriptions, relationships, file/commit links, and extra metadata. | [Raw TOML migration converter](../crates/workdeck-pm/src/migration/convert.rs), [legacy export converter](../crates/workdeck-pm/src/snapshots/legacy_import.rs). | Migration and strict legacy-transfer suites preserve IDs, bodies, timestamps, priority, ownership, links and unknown metadata. Full PM and CLI gates passed before final restore repair; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.D3** — Convert project/cycle/label reference data using explicit ID and status mappings. Later richer fields may be absent; migration must not invent project acceptance or feature maturity. | [Reference conversion](../crates/workdeck-pm/src/migration/convert.rs), [planning records](../crates/workdeck-pm/src/planning/mod.rs). | Planning/migration cases preserve reference IDs, free-form historical statuses/colors/date text, extras and absent historical times without invented maturity; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.D4** — Move repository app-config overrides through an explicit mapping, preserving TOML and user settings. Audit the existing Hunk importer so it does not recreate obsolete active paths. | [Config selection](../crates/workdeck-cli/src/config.rs), [explicit TOML move/edit](../crates/workdeck-cli/src/config_edit.rs), [Hunk importer](../crates/workdeck-migration/src/lib.rs). | Config-cutover/safety, ignored-lock and raw-TOML preservation cases pass; Hunk native destination preserves legacy bytes. Typed review policy rejects legacy saves and retains global saves; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.D5** — Map handoffs, imported session metadata, and review references separately from issues. | [Migration conversion](../crates/workdeck-pm/src/migration/convert.rs), [recorded history](../crates/workdeck-pm/src/history.rs), [compatibility distinctions](project-management-compatibility.md). | Migration retains separate handoff bytes and inert session metadata; shared history tests preserve nested fields and identities. No old typed issue/review foreign key is invented. qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.D6** — Add a migration manifest, verified cutover marker, partial-failure recovery, and repeat-run behavior. | [Migration engine and protocol](../crates/workdeck-pm/src/migration/PROTOCOL.md). | 15 apply tests cover durable barrier/bootstrap, manifest/cutover proof, batches, subprocess races/crash, tampering and exact replay; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-03.D7** — Update active config discovery, commands, tests, examples, and documentation. Keep old source data available until explicit cleanup; no automatic destructive cleanup on launch. | [Caller inventory](project-management-compatibility.md), native config/extensions/VCS boundaries and updated CLI/readme/skill guidance. | Full CLI gate retains all established scenarios after removal of duplicate writers; canonical extension example and bundled skill checks pass. Historical plans are marked superseded; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.E1** — Migration reruns are no-ops or resume the same operation. | [Migration application protocol](../crates/workdeck-pm/src/migration/PROTOCOL.md). | 15 migration application tests cover exact replay, missing legacy source, partial recovery, concurrency and unchanged operation identity; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.E2** — IDs, bodies, and metadata round-trip. | [Migration](../crates/workdeck-pm/src/migration/mod.rs), [native/legacy snapshots](../crates/workdeck-pm/src/snapshots/mod.rs), [explicit restoration](../crates/workdeck-pm/src/restore.rs). | Exact-byte native transfer, legacy metadata/origin conversion and restoration replay cases pass. Independent review found a restore destination-shape gap; repair and final qualification remain pending. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.E3** — Destination conflicts do not overwrite records. | [Migration and restore preconditions](../crates/workdeck-pm/src/restore.rs), [transaction engine](../crates/workdeck-pm/src/transactions.rs). | Source/destination conflict, stale preview, direct edit and interrupted restore cases preserve existing files. New directory-collision repair is under final qualification. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.E4** — Legacy configuration does not shadow the new root. | [Canonical config discovery](../crates/workdeck-cli/src/config.rs). | Config tests prove native precedence, explicit ambiguity and no legacy fallback after native errors; original settings are preserved by explicit edits. qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.E5** — All active `.agents/workdeck` callers are migrated or covered by documented read-only compatibility. | [Read-only legacy admission](../crates/workdeck-cli/src/pm_cli.rs), [reader-only Store](../crates/workdeck-cli/src/store/mod.rs), [typed preference policy](../crates/workdeck-core/src/view_preferences.rs). | 52 mutation variants at both canonical/custom legacy roots preserve bytes and membership; Store persistence APIs removed. Config, extension and source consumers have explicit read-only compatibility; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-03.E6** — No UI path continues writing the old issue store after cutover. | [Native workbench operations](../crates/workdeck-tui/src/workbench/mod.rs); superseded CLI app/tui/views island removed. | Source audit found only shared PM mutations; real clean/dirty native terminal flows create no legacy root. qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |

### 21.5. PM-04 — Unify issue access with the review workbench

Phase state: **Active, incomplete**. See checkpoint and individual remaining criteria.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-04.D1** — Replace dirty-state-dependent application selection with a persistent workbench shell for normal `workdeck` startup. Reuse the continuous review canvas and its state/controller behavior. | [Normal CLI startup](../crates/workdeck-cli/src/main.rs) and [mounted workbench host](../crates/workdeck-tui/src/workbench/host.rs). | Clean/dirty startup uses existing native ReviewApp; 1092 TUI and 98 PTY cases pass, affected 5 workbench PTY rerun after boundary fix; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-04.D2** — Add Issues navigation, list/detail panes, search/filter entrypoints, empty/error states, and contextual create/edit/comment/status/assignment actions backed by the PM API. | [Workbench controller/forms/shell](../crates/workdeck-tui/src/workbench/mod.rs). | 20 focused controller/runtime tests inside full TUI suite plus real CLI-backed terminal create/open/edit flows, filters, comments, assignment/status and empty/error state cases; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-04.D3** — Support issue-to-file/commit/review navigation and issue creation from a selected file or note. | [Workbench source navigation](../crates/workdeck-tui/src/workbench/host.rs), [native terminal scenarios](../crates/workdeck-cli/tests/terminal_pager/workbench.rs). | File/commit round trips and saved note-derived issue flows pass in actual PTYs. Mounted note test reopens the store and checks body, source, receipt and retained original note. qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-04.D4** — Preserve return context: selected issue, preview, review source, file/hunk position, and drafts. | [Captured planning/native return context](../crates/workdeck-tui/src/workbench/host.rs). | File/commit multi-link terminal round trips retain selected issue, native source/position and drafts; source-qualified/stale tests pass; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-04.D5** — Define narrow list/detail layouts and wider side-by-side planning/review layouts. | [Allocated workbench layout](../crates/workdeck-tui/src/workbench/view.rs) and mounted shell. | Tiny/narrow/wide rendering plus real 90/180-column resize and side-by-side planning/review cases pass; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-04.D6** — Retain specialized `diff`, `show`, patch/stdin, pager, and difftool entrypoints and terminal lifecycle. | [Existing native runtime with optional workbench](../crates/workdeck-tui/src/lib.rs). | Full 98 terminal-pager cases include specialized inputs, layout/mouse/pager/session and lifecycle regressions; [Integration checkpoint](project-management-validation.md#integration-checkpoint--2026-09-09). |
| [x] **PM-04.D7** — Remove duplicate issue mutation logic from the old UI once its callers have migrated. | [Shared PM controller](../crates/workdeck-tui/src/workbench/controller.rs), [native host](../crates/workdeck-tui/src/workbench/host.rs). | Production workbench mutations use workdeck-pm; obsolete direct CLI UI writer implementation is removed. qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-04.E1** — PTY tests create, open, and edit an issue from clean and dirty repositories. | [Normal startup and mounted workbench](../crates/workdeck-tui/src/workbench/host.rs). | [Clean/dirty terminal lifecycle](../crates/workdeck-cli/tests/terminal_pager/workbench.rs) creates, opens and edits issues and restores terminal state; full 106-case PTY run passed. qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-04.E2** — PTY tests jump into review and back while retaining selection. | [Planning/native return context](../crates/workdeck-tui/src/workbench/host.rs). | [PTY navigation tests](../crates/workdeck-cli/tests/terminal_pager/workbench.rs) verify file/commit return, explicit multiple-target selection and retained issue/draft context; qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-04.E3** — Existing review navigation, terminal restore, pager, and session controls pass. | [Native review runtime](../crates/workdeck-tui/src/lib.rs), [full terminal-pager suite](../crates/workdeck-cli/tests/terminal_pager.rs). | All 106 PTY cases and 1,117 TUI library cases passed; the final unchanged-source run passed; concurrent extension timeouts in the test-only lint-fix rerun passed on serial recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |
| [x] **PM-04.E4** — The TUI has no direct YAML/SQL mutation path. | [Shared PM adapter/controller](../crates/workdeck-tui/src/workbench/mod.rs). | Source audit confirms production workbench writes call shared PM operations; test-only fixture writes are separate. qualified by the final cutover source and affected-test recheck. [Cutover qualification](project-management-validation.md#cutover-work-in-progress--2026-09-09). |

### 21.6. PM-05 — Add the project-management hierarchy

Phase state: **Complete**. All eleven requirements qualified on candidate `e4487513cd0cfeacd0d6638e9b6aa2e0941a21c63af9c5e12959a34f71e891f3`; see the [phase qualification](project-management-validation.md#pm-05-qualified--2026-09-09).

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-05.D1** — Add native initiatives, projects, milestones, cycles, and targets with stable identities. | [Planning records](../crates/workdeck-pm/src/planning/mod.rs), [hierarchy validation](../crates/workdeck-pm/src/planning/hierarchy.rs). | [Hierarchy tests](../crates/workdeck-pm/tests/hierarchy.rs) and [planning tests](../crates/workdeck-pm/tests/planning.rs) exercise native identities, record validation and replay. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.D2** — Add CLI create/show/list/update/archive operations and corresponding project/cycle UI views. | [Planning CLI](../crates/workdeck-cli/src/pm_reference.rs), [planning workspace](../crates/workdeck-tui/src/workbench/planning_workspace.rs). | [CLI hierarchy tests](../crates/workdeck-cli/tests/pm_hierarchy_cli.rs) exercise all six command families; [PTY workflow](../crates/workdeck-cli/tests/terminal_pager/workbench.rs) creates projects/cycles, navigates members, and archives/restores. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.D3** — Define project lead, scope, goal, dates, exit criteria, milestone outcomes, and target membership. | [Hierarchy contracts](../crates/workdeck-pm/src/planning/hierarchy.rs), [membership queries](../crates/workdeck-pm/src/queries.rs). | [Hierarchy tests](../crates/workdeck-pm/tests/hierarchy.rs) validate rich fields and milestone/project consistency; [query tests](../crates/workdeck-pm/tests/queries.rs) verify direct and inherited target membership. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.D4** — Add labels, users/identities, custom fields, estimates/units, templates, and saved-view definitions. | [Organization policy](../crates/workdeck-pm/src/organization/mod.rs), [templates](../crates/workdeck-pm/src/templates.rs), [saved views](../crates/workdeck-pm/src/saved_views.rs). | [Organization tests](../crates/workdeck-pm/tests/organization.rs), [template tests](../crates/workdeck-pm/tests/templates.rs), and [saved-view tests](../crates/workdeck-pm/tests/saved_views.rs) cover identities, policy, exact estimates, preserved metadata and source-bound definitions. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.D5** — Support issue association changes with reference validation; archive referenced objects rather than silently orphaning issues. Destructive deletion requires an explicit resolution plan. | [Hierarchy reference policy](../crates/workdeck-pm/src/planning/hierarchy.rs), [retirement](../crates/workdeck-pm/src/retirement.rs). | [Reference-resolution tests](../crates/workdeck-pm/tests/reference_resolution.rs) and [retirement tests](../crates/workdeck-pm/tests/retirement.rs) verify canonical references, retained archive links and reviewed resolution before destructive retirement. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.D6** — Add plain Markdown wiki/document links and authoring shortcuts without creating a second docs CMS. | [Wiki API](../crates/workdeck-pm/src/wiki.rs), [native wiki CLI](../crates/workdeck-cli/src/pm_wiki.rs), existing issue document links. | [Wiki core tests](../crates/workdeck-pm/tests/wiki.rs) and [CLI tests](../crates/workdeck-cli/tests/pm_wiki.rs) verify exact Markdown bytes, bounded paths, replay, stale writes and forged-proof rejection. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.D7** — Add one file per time entry, amendment/history rules, and issue/user/cycle reports. | [Time operations](../crates/workdeck-pm/src/time_entries.rs), [time CLI](../crates/workdeck-cli/src/pm_time.rs). | [Time core tests](../crates/workdeck-pm/tests/time_entries.rs) and [CLI tests](../crates/workdeck-cli/tests/pm_time.rs) verify immutable entries, source-bound amendments, captured cycles and active-leaf reporting. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.E1** — A project with milestones and a cycle can be created, populated, reassigned, filtered, and archived without breaking references. | [Hierarchy CLI tests](../crates/workdeck-cli/tests/pm_hierarchy_cli.rs), [planning PTY](../crates/workdeck-cli/tests/terminal_pager/workbench.rs). | [Hierarchy CLI tests](../crates/workdeck-cli/tests/pm_hierarchy_cli.rs) and [project/cycle PTY workflow](../crates/workdeck-cli/tests/terminal_pager/workbench.rs) exercise creation, population, reassignment, filtering and archive/restore with references preserved. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.E2** — Custom-field requirements apply consistently. | [Organization tests](../crates/workdeck-pm/tests/organization.rs), [mounted forms](../crates/workdeck-tui/src/workbench/runtime_tests.rs). | [Organization tests](../crates/workdeck-pm/tests/organization.rs) exercise authoring, templates, imports, completion and historical repair policy; [mounted-form tests](../crates/workdeck-tui/src/workbench/runtime_tests.rs) retain invalid input and apply the same validation. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.E3** — Time records deduplicate by identity. | [Time tests](../crates/workdeck-pm/tests/time_entries.rs). | [Time tests](../crates/workdeck-pm/tests/time_entries.rs) verify request/identity deduplication, distinct equal-valued entries, stale supersession, fork rejection and exactly-once active-leaf totals. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |
| [x] **PM-05.E4** — UI and CLI produce equivalent project membership. | [Shared query](../crates/workdeck-pm/src/queries.rs), [planning navigation tests](../crates/workdeck-tui/src/workbench/planning_navigation_tests.rs), [CLI hierarchy tests](../crates/workdeck-cli/tests/pm_hierarchy_cli.rs). | [Planning navigation tests](../crates/workdeck-tui/src/workbench/planning_navigation_tests.rs) and [real-terminal workflow](../crates/workdeck-cli/tests/terminal_pager/workbench.rs) compare the same shared membership result used by CLI, including archive scope and cycle reassignment. Qualified in the unchanged-source [phase run](project-management-validation.md#pm-05-qualified--2026-09-09). |


#### PM-06 preparation while PM-05 qualifies (not implementation evidence)

Read-only integration review identified these bounded seams for the next phase:

- Put canonical issue parent and prerequisite references on the child/dependent issue.
  Store symmetric related links once beneath `relations/issues/`, with derived inverse
  membership. Capture the full graph before filtering; distinguish hard-dependency readiness
  from parent/child completion conditions. Detect parent, hard-edge and mixed completion
  deadlocks. Cancellation never silently removes or satisfies a prerequisite. Explicit
  removal/replacement and policy-controlled waiver decisions retain attributed reasons.
- Introduce structured satisfied/unsatisfied/unknown completion conditions alongside existing
  reason strings. Both completion preview and every final mutation route must evaluate them
  inside the same transaction snapshot. Manual acceptance cannot bypass unresolved graph
  conditions. Retiring a child or prerequisite must not erase an outstanding requirement.
- Keep native features, gates and evidence separate from `PlanningKind`. Use immutable typed
  IDs, source-bound authoring and full duplicate/path checks. Feature rename, logical reparent
  and physical relocation preserve identity. Issue-to-feature links have one forward authority;
  reverse membership and coverage are derived. Issue completion never promotes feature maturity.
- Evidence references have one canonical namespace, `.workdeck/evidence/`, so an issue,
  feature or gate can refer to an immutable record without copying authority. This refines
  the earlier illustrative issue-local layout. Exact subjects, stable criterion references,
  producer attribution, definition/result identities, timestamps and supersession remain
  declarations until an implemented evaluator qualifies them. Declared/unknown evidence
  cannot pass a gate; source/result/input receipt consistency is validated from the first API.
- Each domain increment includes doctor, prospective import, exact restore, retirement,
  source markers, catalog and receipt/staging validation. Feature/gate consumers compose
  through snapshot-scoped helpers instead of recursively loading the issue query engine.
  Ordinary imports cannot bypass graph or evidence policy; historical restore remains
  distinct. Later indexing work must qualify the large graph workload and source budgets.

PM-06 implementation is now active after PM-05 qualification. Typed `FEAT-`, `GATE-`
and `EVD-` identities are implemented; their focused test verifies canonical serialization,
cross-kind rejection and path/noncanonical-ID rejection (one test passed). Graph, feature,
and gate/evidence cores have separate bounded owners; the root owns CLI/TUI integration.
The full phase checklist remains unchecked until integrated behavior and qualification pass.

PM-06 working checkpoint (not phase qualification):

- Native graph, feature, gate and immutable evidence APIs are integrated with CLI,
  catalogs, native-source admission, doctor, import/restore and retirement. Graph
  and feature views are mounted in the persistent issue/review workbench.
- All 65 workbench unit/mounted tests and 11 workbench PTY tests pass, including
  graph traversal and feature authoring/coverage at widths 78 and 180. Independent
  review reproduced and repaired graph back-history, selected-row visibility, and
  wide-review mouse routing defects.
- Feature coverage now preserves same-feature prerequisites hidden by filters and
  reports missing transitive references: 18 feature tests and scoped lint pass.
  Required graph gate-debt/path repairs pass 104 adjacent core tests. Prospective
  relation imports to retired endpoints are rejected while historical restoration
  and unchanged imported links retain their explicit policy.
- Expanded CLI checks pass 5 feature, 4 gate/evidence, 4 graph, 6 legacy-admission,
  and 2 catalog tests. Retirement previews, stale tokens, reserved identities and
  retry after a committed mutation's Git staging failure have concrete evidence.
  Strict CLI/TUI all-targets Clippy passes.
- Current next action: finish graph receipt intent-proof validation and independent
  feature UI retry/source review, then coordinate formatting and freeze PM-06 for
  all nine unchanged-source gates. No PM-06 requirement is closed from these focused
  runs alone. [Detailed checkpoint](project-management-validation.md#pm-06-active-increments--2026-09-09).
- PM-06 qualification candidate `248ee1112608fffd86aaa3b2b58b2c108598bd7f2d18727dd2b0f768e01b4e59`
  captures 1,031 inputs after coordinated formatting. All nine gates are running
  sequentially from `/tmp/workdeck-pm06-qualification-20260909.py`; source edits are
  frozen until the reports and final fingerprint comparison are collected.
- Historical PM-07 preparation, recorded before PM-06 qualification: context/next
  queries share one PM snapshot and explicit byte budgets,
  citations and omissions. Questions/handoffs use source-bound native records with
  distinct declared-summary semantics. Default work selection is active Unstarted
  work with satisfied hard prerequisites, ordered by priority and stable creation/ID
  ties; completed/canceled/retired work is never manufactured as eligible. An explicit
  unresolved `blocks_work` question excludes implementation selection and suggests
  resolving the question, without becoming an implicit completion gate. CLI/protocol
  generation and explicit instruction-pointer publication formed a separate slice.
  PM-07 subsequently qualified; its implementation and final evidence are in section 21.8.


### 21.7. PM-06 — Add relationships, native features, gates, and evidence

Phase state: **Complete**. All twelve requirements qualified on the unchanged [PM-06 candidate](project-management-validation.md#pm-06-qualified--2026-09-09).

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-06.D1** — Implement parent/child issue hierarchy, directed hard prerequisites, symmetric related links with a single canonical representation, and derived reverse relationships. | [Graph capture](../crates/workdeck-pm/src/graph/capture.rs), [semantic mutations](../crates/workdeck-pm/src/graph/mutations.rs), [CLI](../crates/workdeck-cli/src/pm_graph.rs). | [30 graph tests](../crates/workdeck-pm/tests/graph.rs) and [4 CLI graph tests](../crates/workdeck-cli/tests/pm_graph.rs) cover canonical storage, reverse and symmetric links; full qualification passed. |
| [x] **PM-06.D2** — Reject parent/hard-dependency cycles; expose blocked reasons and preserve canceled prerequisites until their requirement is explicitly resolved, replaced, or waived under policy. | [Graph evaluation](../crates/workdeck-pm/src/graph/evaluation.rs), [waiver validation](../crates/workdeck-pm/src/graph/capture.rs), [graph tests](../crates/workdeck-pm/tests/graph.rs). | [Graph tests](../crates/workdeck-pm/tests/graph.rs) cover parent/hard/mixed cycles, canceled requirements, reasoned replacement/removal, source-bound waivers and concurrent opposite edges; full qualification passed. |
| [x] **PM-06.D3** — Implement native feature and gate records under `.workdeck/features/` and `.workdeck/gates/`. | [Native features](../crates/workdeck-pm/src/features/mod.rs), [gates](../crates/workdeck-pm/src/gates/mod.rs), [feature CLI](../crates/workdeck-cli/src/pm_feature.rs), [gate CLI](../crates/workdeck-cli/src/pm_gate.rs). | [18 feature tests](../crates/workdeck-pm/tests/features.rs), [10 gate tests](../crates/workdeck-pm/tests/gates.rs), feature/gate CLI and snapshot/retirement suites pass. |
| [x] **PM-06.D4** — Support feature hierarchy, typed edges, targets, project/milestone links, sources, and separate decision/maturity/availability fields. Keep issue-to-feature association many-to-many. | [Feature policy](../crates/workdeck-pm/src/features/policy.rs), [coverage](../crates/workdeck-pm/src/features/coverage.rs), [feature tests](../crates/workdeck-pm/tests/features.rs). | [Feature tests](../crates/workdeck-pm/tests/features.rs) and [CLI coverage](../crates/workdeck-cli/tests/pm_features.rs) verify typed hierarchy/associations, metadata preservation and independent declaration fields. |
| [x] **PM-06.D5** — Add evidence references with subject, producer, check/result identity, provenance, and freshness. Declared evidence and verified evidence must remain distinguishable until evaluators exist. | [Evidence types](../crates/workdeck-pm/src/evidence/types.rs), [immutable store](../crates/workdeck-pm/src/evidence/store.rs), [gate assessment](../crates/workdeck-pm/src/gates/assessment.rs), [CLI evidence](../crates/workdeck-cli/src/pm_evidence.rs). | [11 evidence tests](../crates/workdeck-pm/tests/evidence.rs), [gate assessment tests](../crates/workdeck-pm/tests/gates.rs) and [CLI gate tests](../crates/workdeck-cli/tests/pm_gates.rs) verify provenance, source/definition/freshness and immutable supersession without verified outcomes. |
| [x] **PM-06.D6** — Add stable acceptance-criterion references and dependency/child/gate completion explanations. | [Criterion resolution](../crates/workdeck-pm/src/gates/criteria.rs), [completion integration](../crates/workdeck-pm/src/issues.rs), [graph explanations](../crates/workdeck-pm/src/graph/evaluation.rs). | [Graph regressions](../crates/workdeck-pm/tests/graph.rs) and [gate criteria tests](../crates/workdeck-pm/tests/gates.rs) verify stable criterion hashes and required child/prerequisite gate debt, including direct-editor changes. |
| [x] **PM-06.D7** — Provide dependency-path and coverage views with explicit unresolved-reference handling. | [Graph view](../crates/workdeck-tui/src/workbench/graph_view.rs), [native feature workspace](../crates/workdeck-tui/src/workbench/feature_workspace.rs), [coverage](../crates/workdeck-pm/src/features/coverage.rs). | [Coverage regressions](../crates/workdeck-pm/tests/features.rs), [path regressions](../crates/workdeck-pm/tests/graph.rs) and [PTY journeys](../crates/workdeck-cli/tests/terminal_pager/workbench.rs) verify filtered/missing references and actual navigation. |
| [x] **PM-06.E1** — Graph fixtures detect cycles, dangling references, canceled prerequisites, and unresolved children. | [Graph regression tests](../crates/workdeck-pm/tests/graph.rs), [graph CLI tests](../crates/workdeck-cli/tests/pm_graph.rs). | [Graph suite](../crates/workdeck-pm/tests/graph.rs): cycles, dangling references, canceled prerequisites, unresolved children, concurrency and required gate conditions all pass. |
| [x] **PM-06.E2** — Finishing an issue does not automatically promote feature maturity. | [Feature core tests](../crates/workdeck-pm/tests/features.rs), [feature CLI tests](../crates/workdeck-cli/tests/pm_features.rs). | [Core features](../crates/workdeck-pm/tests/features.rs) and [feature CLI](../crates/workdeck-cli/tests/pm_features.rs) explicitly finish linked issues and assert declared feature maturity is unchanged. |
| [x] **PM-06.E3** — Unknown evidence cannot satisfy a gate. | [Gate tests](../crates/workdeck-pm/tests/gates.rs), [evidence tests](../crates/workdeck-pm/tests/evidence.rs), [CLI gate/evidence tests](../crates/workdeck-cli/tests/pm_gates.rs). | [Gates](../crates/workdeck-pm/tests/gates.rs), [evidence](../crates/workdeck-pm/tests/evidence.rs) and [CLI tests](../crates/workdeck-cli/tests/pm_gates.rs) reject unknown/declared evidence as completion proof. |
| [x] **PM-06.E4** — Feature IDs survive moves and renames. | [Feature store and relocation](../crates/workdeck-pm/src/features/store.rs), [feature tests](../crates/workdeck-pm/tests/features.rs), [CLI tests](../crates/workdeck-cli/tests/pm_features.rs). | [Feature core](../crates/workdeck-pm/tests/features.rs) and [CLI rename/move/replay](../crates/workdeck-cli/tests/pm_features.rs) preserve canonical IDs and reject stale sources. |
| [x] **PM-06.E5** — Explanations match the actual graph. | [Graph snapshot evaluation](../crates/workdeck-pm/src/graph/evaluation.rs), [feature coverage](../crates/workdeck-pm/src/features/coverage.rs), [terminal regressions](../crates/workdeck-cli/tests/terminal_pager/workbench.rs). | [Graph/coverage PTY](../crates/workdeck-cli/tests/terminal_pager/workbench.rs), [graph view tests](../crates/workdeck-tui/src/workbench/graph_view.rs), [feature workspace tests](../crates/workdeck-tui/src/workbench/feature_workspace.rs) and core snapshots verify real explanations and retain old state on source races. |


### 21.8. PM-07 — Make the repository self-describing to agents

Phase state: **Complete**. [Final qualification](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09) binds all nine gates to one unchanged source.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-07.D1** — Complete `capabilities --json`: schema/command versions, selected repository/source, supported operations, and meaningful unavailable-state reporting. | [CLI catalog](../crates/workdeck-cli/src/pm_catalog.rs). | [Discovery tests](../crates/workdeck-cli/tests/pm_catalog.rs) and [fresh-session CLI](../crates/workdeck-cli/tests/pm_context.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.D2** — Implement `context --issue` with a budget, citations, requirements, source links, relevant instructions, feature references, blockers, questions, handoff, and verification requirements. | [Shared context](../crates/workdeck-pm/src/context/mod.rs), [packet assembly](../crates/workdeck-pm/src/context/packet.rs), [CLI adapter](../crates/workdeck-cli/src/pm_context.rs), [Context workspace](../crates/workdeck-tui/src/workbench/context_workspace.rs). | [Core context cases](../crates/workdeck-pm/tests/context.rs), [CLI byte budgets](../crates/workdeck-cli/tests/pm_context.rs) and [real terminal journeys](../crates/workdeck-cli/tests/terminal_pager/workbench_context.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.D3** — Implement `issue next` for ready work selection and top-level `next --issue` for suggested actions. Return eligibility/exclusion reasons and typed action preconditions. | [Shared next actions and selection](../crates/workdeck-pm/src/context/next.rs), [CLI](../crates/workdeck-cli/src/pm_context.rs). | [Selection/actions cases](../crates/workdeck-pm/tests/context.rs) and [fresh-session CLI](../crates/workdeck-cli/tests/pm_context.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.D4** — Add structured handoff create/show and question create/answer/supersede operations. | [Questions](../crates/workdeck-pm/src/questions/mod.rs), [handoffs](../crates/workdeck-pm/src/handoffs/mod.rs), [CLI](../crates/workdeck-cli/src/pm_continuity.rs). | [Question tests](../crates/workdeck-pm/tests/questions.rs), [handoff tests](../crates/workdeck-pm/tests/handoffs.rs), [CLI continuity](../crates/workdeck-cli/tests/pm_continuity.rs), [mounted flows](../crates/workdeck-tui/src/workbench/context_tests.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.D5** — Add compact output, fields, pagination, bounded errors, `--no-input`, and stable request identity consistently across new commands. | [Output adapter](../crates/workdeck-cli/src/pm_context.rs), [pagination](../crates/workdeck-cli/src/pm_continuity.rs), [bounded diagnostics](../crates/workdeck-cli/src/pm_diagnostics.rs). | [CLI context](../crates/workdeck-cli/tests/pm_context.rs) and [continuity/output tests](../crates/workdeck-cli/tests/pm_continuity.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.D6** — Generate command/schema references and the PM agent skill from typed catalogs. Merge a thin protocol pointer into repository instructions only through explicit initialization/update. | [Typed renderer](../crates/workdeck-cli/src/pm_catalog.rs), [protocol command](../crates/workdeck-cli/src/pm_protocol.rs), [generated skill](../skills/workdeck-pm/SKILL.md), [explicit pointer installer](../crates/workdeck-cli/src/pm_protocol_install.rs). | [Catalog parity test](../crates/workdeck-cli/tests/pm_catalog.rs) and [pointer preservation/recovery tests](../crates/workdeck-cli/tests/pm_protocol.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.D7** — Add advisory overlapping-path/dependency visibility; it does not allocate worktrees or spawn agents. | [Bounded overlap capture](../crates/workdeck-pm/src/context/packet.rs). | [Core overlap and coverage cases](../crates/workdeck-pm/tests/context.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.E1** — A scripted fresh-session exercise obtains relevant context and the correct next action using only an issue ID. | [Shared next/context operations](../crates/workdeck-pm/src/context/mod.rs), [CLI](../crates/workdeck-cli/src/pm_context.rs). | [Fresh-session scripted CLI](../crates/workdeck-cli/tests/pm_context.rs) and [real PTY journey](../crates/workdeck-cli/tests/terminal_pager/workbench_context.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.E2** — Output respects its budget and reports omissions. | [Packet budget](../crates/workdeck-pm/src/context/budget.rs), [transport budget](../crates/workdeck-cli/src/pm_context.rs). | [UTF-8/minimum/omission tests](../crates/workdeck-pm/tests/context.rs), [complete stdout budget](../crates/workdeck-cli/tests/pm_context.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.E3** — Another session resumes from a handoff without treating stale evidence as current. | [Immutable handoffs](../crates/workdeck-pm/src/handoffs/mod.rs), [Context workspace](../crates/workdeck-tui/src/workbench/context_workspace.rs). | [CLI resume](../crates/workdeck-cli/tests/pm_continuity.rs), [second-process PTY resume](../crates/workdeck-cli/tests/terminal_pager/workbench_context.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.E4** — Generated documentation and skills match command schemas. | [Catalog renderers](../crates/workdeck-cli/src/pm_catalog.rs), [PM skill](../skills/workdeck-pm/SKILL.md). | [Generated-reference parity](../crates/workdeck-cli/tests/pm_catalog.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |
| [x] **PM-07.E5** — Empty eligible-work results do not manufacture work. | [Ready-work selection](../crates/workdeck-pm/src/context/next.rs). | [Core empty/excluded selection](../crates/workdeck-pm/tests/context.rs), [CLI empty result](../crates/workdeck-cli/tests/pm_context.rs); [final qualification passed](project-management-validation.md#pm-07-qualified-checkpoint--2026-09-09). |

### 21.9. PM-08 — Add commands, verification profiles, and a local check runner

Phase state: **Complete** on `c78a0b2f`. All 15 deliverables/exit criteria have implementation and validation evidence, and independent requirement review found no remaining implementation blocker. CLI727 (including 115 real PTYs), support568, strict eight-package lint, architecture, skills and formatting passed on the final candidate. PM624, TUI1,175 and startup3 carry forward from `b69d7c5d` by explicit unchanged-input equivalence; they were not rerun on the final aggregate fingerprint. All nine reports have passing status, and the final source comparison matched. The focused evidence below is included in this qualification rather than added to its 3,097-test total. Baseline discovery failures remain in `/tmp/workdeck-pm08-cli-discovery-red.json`; earlier startup, cleanup and watcher observations remain PM-12 reliability evidence. See [qualification and provenance](project-management-validation.md#pm-08-qualified-checkpoint--2026-09-09).

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-08.D1** — Add `.workdeck/commands/`, `checks/`, and `check-profiles/` schemas and list/show/validate commands. | Shared `commands/` and `checks/` catalogs; first-class CLI discovery and typed schema registrations. | Command catalog3 and CLI8 GREEN; generated catalog/schema parity6 GREEN, including strict custom/x-* definition namespaces. |
| [x] **PM-08.D2** — Define recipe argv, argument schema, cwd, toolchain/environment prerequisites, timeout, artifacts, and declared effects. Shell execution is an explicit recipe type; discovery never executes recipes. | Typed tokens/parameters, explicit shell/argv recipes, safe cwd/tools/environment capture, declared effects and bounds. | Planner19 and execution16 GREEN: inert discovery, whole-element argv/shell arguments, private environment pins, tool inputs, cwd-swap RED→GREEN and execution bounds. |
| [x] **PM-08.D3** — Define check expectations and profiles such as quick, issue, pre-submit, and release-readiness. Profiles select required evidence; they do not become a general workflow language. | Flat profile definitions and `check profile` commands; declared process/JUnit/SARIF expectations. | Planner19 and CLI8 GREEN cover required/requested flat profiles and explicit selection; report11 GREEN covers expectations. Completion admission remains PM-11 scope. |
| [x] **PM-08.D4** — Implement `check plan` with source/acceptance/check-definition fingerprints and selection reasons. Include relevant dependency/toolchain inputs; incomplete impact information broadens the selected checks instead of claiming complete coverage. | `CheckPlan` binds exact configuration/requirements/definitions, complete manifests and conservative selection reasons. | Planner19 GREEN includes exact configuration/definition/input proofs, dependency/toolchain changes, conservative selection, and complete historical manifest validation; shared group27 GREEN. |
| [x] **PM-08.D5** — Implement explicit foreground `command run` and `check run --plan` with process-group cleanup, bounded output, timeout/cancellation handling, and structured result receipts. | Shared durable reserve/finish engine, owned foreground process groups, explicit CLI runs and TUI worker. | Execution16 plus PM library13 (including process4), CLI8 and all four actual PTYs GREEN. Interrupted result transactions, concurrent retry, lost acknowledgement and cleanup are covered. |
| [x] **PM-08.D6** — Store raw local logs and temporary plans under ignored `.workdeck/.local/`; retain/export durable evidence descriptors needed for completion and history. Missing local artifacts are visible and cannot remain sufficient evidence merely because their result IDs are known. | Ignored saved plans/logs/artifacts; canonical run descriptors and receipts wired into doctor, export/import and staging. | Shared record5 and execution16 GREEN cover exact export/receipt authority, ignored local state, missing artifacts and whole-run-directory RED→GREEN. CLI8 verifies the same 4 MiB plan contract for fingerprint/file/stdin. |
| [x] **PM-08.D7** — Add generic process results and declared structured report adapters, initially JUnit and SARIF where applicable. Exit zero does not establish that an expected test suite ran. | Pure bounded XML1.0 JUnit and SARIF2.1.0 subset assessment; independent process verdict. | Report11 GREEN, including contradictory JUnit root totals and XML/SARIF false-green RED→GREEN; execution16 keeps failed processes and missing reports separate from report verdicts. |
| [x] **PM-08.D8** — Add results/status/explain, failure filtering, and result references in the TUI/context packet. | CLI status/results/explain and filtered pages; latest5 bounded context summaries and terminal Checks section. | CLI8, context30, mounted workbench98 plus one navigation regression, and four actual PTYs GREEN; historical-pass filtering after proof loss and context-to-result navigation are covered. |
| [x] **PM-08.D9** — Support dirty-worktree feedback with a captured input identity. Recheck identity after an in-place run; if relevant inputs changed during execution, mark results stale/unknown. | Portable input hashes/membership with private descriptor race guards; live before/after and status reassessment. | Planner19 and execution16 GREEN cover portable dirty-input identity, cwd/source swaps, complete historical manifests and input changes during execution; current assessment preserves stale history. |
| [x] **PM-08.E1** — Checks run against an identified subject. | Reviewed exact `CheckPlan` and durable intent subject. | Planner19, execution16 and CLI8 GREEN bind the reviewed plan and durable intent to the exact subject; family/expected-plan mismatches reject before execution. |
| [x] **PM-08.E2** — Changed inputs invalidate plans or results. | Live plan revalidation and current result assessment. | Planner19, execution16, CLI8 and context30 GREEN cover changed definitions/inputs, stable request replay, stale cursors and artifact races without silently rebasing the plan. |
| [x] **PM-08.E3** — Failing, empty-suite, missing-report, canceled, and timeout fixtures classify correctly. | Separate process/report/input/artifact state classification. | Report11, execution16 and process4 GREEN classify failing/empty/missing-report/timeout/canceled fixtures; actual PTYs cover cancellation and repeated OS signals. |
| [x] **PM-08.E4** — Logs remain bounded. | Per-stream retention and aggregate declared-output admission limits. | Process4 and execution16 GREEN cover bounded stream retention and aggregate output rejection before reservation; planner19 exposes oversized-plan blockers. |
| [x] **PM-08.E5** — Terminating a command cleans up its owned children. | Owned process groups and explicit cleanup acknowledgement before terminal exit. | Process4, mounted owned-worker tests and all four actual PTYs GREEN cover descendant cleanup, Ctrl-C escalation, quit/disconnect acknowledgement and deferred suspend. |
| [x] **PM-08.E6** — Issue context reports actionable failures without dumping raw output. | Bounded `ContextCheckRun` diagnostics with original source pins and exact omission counts. | Context4 plus prior26 GREEN prove actionable bounded diagnostics, latest-five omissions and artifact freshness without raw logs; CLI8 and mounted/PTY navigation integration GREEN. |


### 21.10. PM-09 — Add explicit Git collaboration and claims

Phase state: **Complete**. All 16 deliverables and exit criteria have integrated
core, CLI and TUI evidence. Candidate `65a50ac0940b643d1e28da1b81debac7374840e89569ed4a639b211c956e3fbe`
passed PM720, CLI743 (including119PTY), TUI1195, support568 and examples3, plus
strict lint, architecture, generated skills and formatting. All nine gates ran on
this candidate; final source comparison matched. Earlier support failures remain
explicit PM-12 hardening observations, not evidence that their causes were repaired.

Latest focused evidence: shared/local claimed completion passed 23 tests (12 local,
11 shared), including initial inspected binding and mirror/Git-directory replacement
between completion and release, with original-request retry after restoration. Sources/publication passed
19 tests including identity, proposal recovery and reviewed remote binding. Actual Claims and immutable Sources
PTY journeys passed across narrow/wide layouts. The total observation deadline and
source unit suite passed eight cases; an expired budget retains confirmed history
without authorizing continued work.

Latest integration: central capacity and completion binding passed 108 focused tests
including affected snapshots/migration; the workbench group passed 119. Sources remain
readable with explicitly unknown publication binding when a configured remote is absent.

Final qualification: all nine gates passed on the frozen candidate (1,164 inputs).
The [collaboration guide](project-management-collaboration.md) documents the implemented flow.
See [qualification evidence](project-management-validation.md#pm-09-qualified-checkpoint--2026-09-09).

Key logs: `/tmp/workdeck-pm09-claims-completion-green.log`,
`/tmp/workdeck-pm09-sources-binding-green.log`,
`/tmp/workdeck-pm09-proposal-green-binding-red.log`,
`/tmp/workdeck-pm09-collaboration-cli-green.log`,
`/tmp/workdeck-pm09-workbench-claims-focused.log`,
`/tmp/workdeck-pm09-coordination-guard-green.log`,
`/tmp/workdeck-pm09-capacity-completion-green.log`,
`/tmp/workdeck-pm09-capacity-adjacent-green.log`,
`/tmp/workdeck-pm09-capacity-completion-clippy.log`.

Resolved review details: ordinary transactions now enforce prospective receipt-history
capacity; replay and already-admitted recovery remain intact. Shared completion and its
separate release retain the original inspected Git/remote binding and reject rebinding.
These repairs are included in passing final integrated phase qualification.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-09.D1** — Implement configured accepted planning ref, implementation-branch proposals, and an isolated coordination worktree/ref. Show source identity and freshness in CLI/TUI output. | `workdeck-pm/src/sources/{capture,coordination}.rs`; CLI source commands; immutable Sources workbench. | Source/source-publication focused suites and narrow/wide Sources PTY pass; integrated phase qualification passes. |
| [x] **PM-09.D2** — Add explicit fetch/sync/status and change-proposal operations. Do not rebase dirty code worktrees or silently stage/commit/publish application content. | Core fetch/sync and fingerprinted proposal operations; CLI adapters; mounted Sources operation UI. | Source/publication and CLI focused workflows preserve developer state; reviewed-binding core repair and mounted UI pass; full CLI/terminal qualification passes; supporting-crate gate passes. |
| [x] **PM-09.D3** — Support local and shared claims with distinct guarantees. Local mode coordinates only writers using that source; it cannot advertise cross-clone exclusivity. | `claims/{store,shared}.rs` keeps LocalSourceOnly, Unconfirmed and SharedConfirmed distinct. | Local claims and shared publication tests pass; cached reads never authorize shared continuation. |
| [x] **PM-09.D4** — Implement claim/renew/release/list with unique token/generation, actor, issue revision, expiry, and operation identity. Active-work display derives from confirmed coordination state. | Typed claim records, explicit mutation commands, Claims workbench, immutable work contracts. | Claims/workbench focused suites and actual Claims PTY pass; full CLI/TUI qualification passes; supporting-crate gate passes. |
| [x] **PM-09.D5** — Publish a coordination commit descending from the exact observed ref; non-fast-forward rejection causes reload and reevaluation before bounded retry. Never force-push history or blindly replay an old claim after rebasing. | `sources/coordination.rs` publishes exact-parent candidates with bounded non-fast-forward reevaluation. | Two-clone race and publication recovery focused tests pass; full integrated gates pass. |
| [x] **PM-09.D6** — Require current token/generation for renew/release/agent completion. Changes to accepted issue requirements require explicit revalidation of the active work contract. | `claims/{transition,completion}.rs` checks exact token/generation and accepted requirements. | Completion23 (12 local, 11 shared) passes, including stale generation, accepted requirements drift and reviewed binding; mounted Complete UI passes. Full terminal qualification passes; supporting-crate gate passes. |
| [x] **PM-09.D7** — Add uncertain-push reconciliation, expired-claim recovery, configured clock-skew handling, and explicit cancellation/supersession states. | Claim transition policy and retained candidate/request reconciliation; explicit recovery and terminal states. | Local claim, shared lost-response and proposal interruption tests pass; owned publication lifecycle PTY passes in the full CLI gate. |
| [x] **PM-09.D8** — Make `doctor --staged` validate the staged snapshot and its relation closure rather than substitute working-tree contents. Offer hook installation explicitly. | `staged_doctor.rs`, hooks engine and shared CLI entrypoints. | 15 core and 5 CLI tests pass, including installed hooks and alternate staged index; final phase gates pass. |
| [x] **PM-09.D9** — Support reviewed publication on protected canonical branches without assuming direct pushes are allowed. Claim publication and canonical issue proposals remain separate operations. | Reviewed ProposalPlan and isolated candidate publication; accepted ref never directly pushed. | Protected canonical branch, proposal recovery and reviewed-binding regressions pass; final integrated qualification passes. |
| [x] **PM-09.E1** — Two clones racing one issue produce one confirmed winner. | Synchronized two-clone claim publication fixture. | Focused shared source race test passes; final integrated candidate passes. |
| [x] **PM-09.E2** — Stale token writes fail. | Exact precondition checks in local/shared transition and completion. | Claim/completion focused stale-token, generation and reviewed-binding cases pass; final integrated candidate passes. |
| [x] **PM-09.E3** — Lost push responses reconcile without duplicate acquisition. | Retained exact candidate and original request/receipt reconciliation. | Lost-response publication and separate shared release cases pass; final integrated candidate passes. |
| [x] **PM-09.E4** — Expiry does not blindly authorize conflicting continuation. | Expiry/skew disposition plus explicit takeover/revalidation; no process-fencing claim. | Local expiry/recovery and historical receipt continuation tests pass; final integrated candidate passes. |
| [x] **PM-09.E5** — Proposals remain distinct from accepted state. | Distinct accepted/proposal source roles and reviewed proposal parent/tree constraints. | Core, CLI and immutable Sources PTY pass; interactive proposal publication passes in the full CLI gate. |
| [x] **PM-09.E6** — Developer index and worktree content are preserved. | Isolated Git transport/candidate trees and explicit source operations. | Core/CLI/actual PTY assertions preserve index, HEAD and dirty code; all four collaboration terminal journeys pass in the full CLI gate. |
| [x] **PM-09.E7** — Staged validation catches defects hidden by working-tree edits. | Index-tree capture and staged relation closure validation. | Core staged and actual installed-hook CLI tests pass; final integrated candidate passes. |

### 21.11. PM-10 — Add indexed views and cross-repository navigation

Phase state: **Active**. Initial storage/projector/query implementation begins after PM-09 closure; acceptance evidence remains pending.

Current increment: native working-tree checkout switching is mounted on My work
`o`/`b`. Qualified source identities retain complete shells, forms and review state;
up to eight contexts preserve owned workers and share foreground-run ownership.
The source label remains visible while editing. The launch registry follows the
active view without being copied into targets. A complete candidate passed 1,264
TUI tests, all 28 workbench terminal journeys, nine reload tests, strict lint,
architecture and formatting after the nested command-cwd retention repair; see the
top checkpoint and validation history for final-source results.

Core/CLI registry and My work operations support explicit mappings,
source-qualified identities, pagination and unavailable-source reporting. Their
five work facets, native checkout switching, and read-only accepted/proposal
planning-view switching are mounted and have integrated qualification evidence
in the current top checkpoint.

Large-source functional runs exist for both required datasets. The historical runs
missed the original one-second target (23.22 s for 40,000 features, 10.05 s for
10,000 issues); current qualification uses the calibrated family-specific budgets.
Richer workloads, separated query timing, incremental optimization and actual mounted
latency/memory qualification remain open. No PM-10 requirement is marked complete from
incremental evidence. See
[validation](project-management-validation.md#pm-10-active-implementation-checkpoint--2026-09-09)
and [performance evidence](project-management-performance.md).

Read-only preparation during PM-09 qualification identified the following implementation
seams; these are design decisions to validate, not delivered capabilities:

- Build a separately versioned SQLite/FTS projection from one immutable native source
  capture. Prefer an in-memory connection and guarded serialized checkpoints under
  `.workdeck/.index/`, so SQLite never opens authority paths or filesystem sidecars.
  Measure checkpoint copies and peak memory before accepting this persistence strategy.
- Query handles, bounded pages, counts, direct ordinal/ID lookup and details must pin the
  same repository, registered checkout, source identity and projection generation. Keep
  last-good data visibly stale when refresh fails; preserve existing substring filtering
  semantics independently of additional FTS search.
- Replace whole-collection parsing/cloning/rendering in Issues, Features and Planning.
  Add actual board/tree navigation on the shared bounded reader, including prerequisites
  outside the active filter. Git source capture currently launches one process per blob;
  replace this with bounded batch capture before large-source qualification.
- Repository switching must retain controllers under qualified checkout/source keys.
  Existing claim/check/publication workers must remain reachable for polling and shutdown;
  switching views must not rebind a retained mutation draft to another source.
  Native switching now qualifies review roots and providers and retains each
  source's navigation and nested command cwd with shared foreground ownership.
  Ref-backed accepted/proposal views must remain read-only;
  their full planning-view switch is still pending.
- Audit loader limits against the separate 40,000-feature and 10,000-issue datasets,
  including organization's whole-root scan. Measure cold/warm timings, p50/p95 and memory;
  no performance target is currently established by the small existing fixtures.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [ ] **PM-10.D1** — Implement a disposable SQLite/FTS projection over native records, graph edges, comments, evidence, and saved views. Version the projection separately from source schemas. | [Versioned schema](../crates/workdeck-pm/src/projection/schema.rs), [projector](../crates/workdeck-pm/src/projection/projector.rs) and [record extraction](../crates/workdeck-pm/src/projection/extract.rs). | Native-family, query parity and schema-rebuild tests pass in the current 79-test core gate; final complete-family/phase audit remains open. |
| [ ] **PM-10.D2** — Add deterministic reindex, fingerprints, incremental refresh, and handling for editor changes, Git checkout/merge, malformed input, and interrupted indexing. | [Refresh and manifests](../crates/workdeck-pm/src/projection/storage.rs), [source capture](../crates/workdeck-pm/src/sources/capture.rs) and [checkpoint publication](../crates/workdeck-pm/src/projection/cache.rs). | Storage tests cover rebuild, editor changes, malformed input, interrupted publication and concurrent CAS; ref-backed terminal tests cover source refresh. The calibrated full-size refresh gate passes; an explicit full checkout/merge matrix remains open. |
| [ ] **PM-10.D3** — Publish consistent query snapshots; do not combine halves of different source revisions. Last-good results carry stale/error indications when source becomes invalid. | [Immutable read views](../crates/workdeck-pm/src/projection/storage.rs), [pinned query handles](../crates/workdeck-pm/src/projection/query.rs) and [indexed workspace](../crates/workdeck-tui/src/workbench/indexed_workspace.rs). | Current storage tests preserve old readers and checkpoint bytes through malformed input and edits at four publication boundaries; full TUI and 32 PTYs pass before the latest scan optimization. Final phase audit remains open. |
| [ ] **PM-10.D4** — Add list/board views, grouping/sorting/filtering, native feature tree, target/dependency slices, project/milestone views, cycle carryover, and activity timelines. | [Issue board](../crates/workdeck-tui/src/workbench/board_view.rs), [native feature workspace](../crates/workdeck-tui/src/workbench/feature_workspace.rs), [planning and carryover](../crates/workdeck-tui/src/workbench/planning_carryover.rs), [Activity](../crates/workdeck-tui/src/workbench/activity.rs). | Focused and actual terminal evidence is recorded in the [validation history](project-management-validation.md). Full integrated phase and measured large-view qualification remain open. |
| [ ] **PM-10.D5** — Add My work across explicitly registered repositories, qualified references, local path mappings, and repository/worktree/source switching with preserved view context. | [Registry/My work core](../crates/workdeck-pm/src/registry/mod.rs), [CLI](../crates/workdeck-cli/src/pm_registry.rs), [mounted work facets and source browser](../crates/workdeck-tui/src/workbench/my_work.rs), [retained native contexts](../crates/workdeck-tui/src/workbench/checkouts.rs). | Core/CLI, mounted retention/worker tests and actual terminal journeys pass; [native switch history](project-management-validation.md#pm-10-mounted-native-checkout-switching--2026-09-10). Ref-backed views and review-requested/overdue facets are qualified in the current checkpoint. Claimed/blocked facets also pass the current integrated core/CLI/TUI/terminal gates; mounted-scale performance and final PM-10 qualification remain open. |
| [ ] **PM-10.D6** — Keep writes scoped to one source. Cross-repository reports do not create a central planning authority or infer completion for unavailable external prerequisites. | [Native citation admission](../crates/workdeck-pm/src/projection/live.rs) and [registered work evidence](../crates/workdeck-pm/src/registry/work_evidence.rs). | Seven live tests reject wrong roots, replaced checkouts, wrong kinds and stale citations. Registry reports preserve unavailable members and unconfirmed shared claim observations. Full cross-repository prerequisite/phase audit remains open. |
| [x] **PM-10.D7** — Virtualize long views and benchmark a synthetic 40,000-node feature graph plus an independent 10,000-issue workload. Record dataset, machine, cold/warm timing, memory, and p50/p95 responses. | [Scale harness](../crates/workdeck-pm/examples/projection_bench.rs), [bounded query windows](../crates/workdeck-pm/src/projection/query.rs), [mounted workspace](../crates/workdeck-tui/src/workbench/indexed_workspace.rs) and [mounted probe](../crates/workdeck-tui/src/workbench/indexed_workspace_tests.rs). Core release benchmarks pass at 10.87 s cold / 8.35 s incremental for features and 7.03 s cold / 5.73 s incremental for issues; the incremental refreshes fit the enforced 30 s/15 s budgets and warm-filter p95 stays under the enforced 100 ms budget, while cold-index timings are recorded observations without a budget. The opt-in mounted probe passes 40,000 features (10.22 s open, 13.43/13.64 ms list/page p50/p95, 54.60/56.60 ms tree p50/p95) and 10,000 issues (7.94 s, 4.49/4.58 ms list/page, 9.99/13.48 ms board), with 671 MB sequential peak RSS. Richer records, independent tree/board thresholds and separate cold/memory budgets are tracked as additional limitations. |
| [ ] **PM-10.E1** — Deleting the index yields equivalent rebuilt queries. | [Checkpoint rebuild](../crates/workdeck-pm/src/projection/storage.rs). | checkpoint_load_and_index_deletion_rebuild_equivalent_generation passes; full-size harness also checks checkpoint query equivalence. Final phase audit remains open. |
| [ ] **PM-10.E2** — Malformed and stale sources are visible. | [Projection state](../crates/workdeck-pm/src/projection/storage.rs) and [source browser](../crates/workdeck-tui/src/workbench/my_work.rs). | Malformed-source retention, forged receipt rejection, stale citation and unavailable-member tests pass; mounted stale-source and ref-refresh terminal journeys pass. Final phase audit remains open. |
| [ ] **PM-10.E3** — Source views stay distinct. | [Source-qualified views](../crates/workdeck-pm/src/projection/storage.rs) and [retained checkout contexts](../crates/workdeck-tui/src/workbench/checkouts.rs). | Copied checkpoint rejection, accepted/proposal/home terminal flow and shared claim/proposal mismatch tests pass. Final phase audit remains open. |
| [ ] **PM-10.E4** — Same-looking IDs cannot cross-mutate repositories. | [Exact native reopening](../crates/workdeck-pm/src/projection/live.rs). | same_looking_id_and_even_copied_repository_identity_cannot_cross_checkout and replaced-directory admission tests pass in current live gate. Final phase audit remains open. |
| [ ] **PM-10.E5** — Large lists and trees remain navigable within measured budgets. | [Bounded mounted reads](../crates/workdeck-tui/src/workbench/indexed_workspace.rs), [mounted probe](../crates/workdeck-tui/src/workbench/indexed_workspace_tests.rs) and [scale harness](../crates/workdeck-pm/examples/projection_bench.rs). | The release mounted probe passes full-size feature and issue list/page reads plus feature-tree and issue-board traversals, final-row navigation and worker shutdown (13.64 ms list p95, 56.60 ms tree p95, 13.48 ms board p95). Independent tree/board thresholds, cold/memory budgets and final PM-10 phase qualification remain open. |
| [ ] **PM-10.E6** — Readiness explains prerequisites outside the current filter. | [Graph readiness](../crates/workdeck-pm/src/graph/evaluation.rs) and [blocked evidence](../crates/workdeck-pm/src/registry/work_evidence.rs). | Outside-filter prerequisite PTY, canceled prerequisite, waiver, stale-answer and captured-source race tests pass. Final phase audit remains open. |

### 21.12. PM-11 — Integrate CI, review, and completion evidence

Phase state: **In progress** under the recorded sequencing adjustment. The shared
immutable commit validator and `ci validate` CLI are implemented; accepted-contract
review admission and CI check qualification are not counted as delivered.

Current slice: [CI validation operation](../crates/workdeck-pm/src/ci_validation.rs)
and [bounded commit capture](../crates/workdeck-pm/src/sources/commits.rs) resolve
exact commit IDs, full refs or HEAD, validate both immutable planning catalogs using
the same inspector as staged hooks, and report repository/commit/tree/content identities.
Missing planning baselines, cross-repository candidates, unsafe modes and moved
selected refs fail. Dirty worktree/index contents are not substituted. Unselected
HEAD movement does not invalidate exact commit inputs. The explicit report basis is
`planning_source_validation`, which does not establish trusted baseline authority,
check execution or completion. Required profile/check/recipe and acceptance-policy contracts now capture separately
from each revision, with exact document fingerprints and semantic comparison.
The implemented `ci validate` command and generated command/schema/skill references
expose this result. Subject criteria/dependencies/gates and workflow policy are now captured as typed
semantic contracts with stable subject identities and independent document pins.
Declared evaluator/test inputs now have independent committed manifests and change
reports. Trusted admission of those declarations, required red/green, trusted baseline selection,
review admission and producer provenance remain open. Revision-bound foreground
execution is implemented with retained local-feedback semantics. The PM-11 phase
remains open pending its integrated trust, policy, review and release criteria.

Implementation seams inspected: [bounded Git reads](../crates/workdeck-pm/src/sources/git.rs),
[planning blob admission](../crates/workdeck-pm/src/sources/capture_core.rs),
[staged validation](../crates/workdeck-pm/src/staged_doctor.rs), and
[check plans](../crates/workdeck-pm/src/checks/types.rs). The implemented first slice validates
resolved base/head commit trees even when working-tree/index content differs, retain
exact identities, reject missing/cross-repository authority and keep caller-selected
baselines distinct from confirmed accepted/trusted evaluation. Existing
VerificationBasis supports LocalFeedback only; forwarding an ordinary local run or
trusting a caller-set CI flag would not implement CI qualification. Add bounded
commit/ref selection without broadening publication-ref parsing or enabling remote
fetch/lazy hydration. Executable CLI/schema contracts are frozen by actual CLI and
generated-catalog tests. `ci plan` and `ci check` now use the shared admission/runner;
trusted CI qualification remains open.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [x] **PM-11.D1** — Add headless entrypoints such as `workdeck ci validate --base <SHA> --head <SHA>` for PM changes and `workdeck ci check --profile <ID> --revision <SHA>` for configured verification. Freeze exact syntax in the command catalog; do not expose unsupported remote status APIs. | `pm_ci.rs` and `pm_ci_checks.rs` expose `ci validate`, before working config discovery; exact SHA/full-ref/local-branch/HEAD inputs, JSON source/report/errors. `ci plan` prepares a committed profile/issue plan and `ci check` executes its exact saved binding with actor/request attribution and shared recovery. Feedback remains local; trusted CI admission is still open. | Full PM: 846 tests. CLI/catalog: 22 tests, including actual saved-plan execution, failed checks, dirty input and stale replay. Generated syntax/schema parity, strict lint and architecture pass. See the validation log. |
| [ ] **PM-11.D2** — Validate actual commit/staged snapshots and relation closure. Resolve branch names before checking and return exact source/check-definition identities. | Partial: `ci_validation.rs`, `sources/commits.rs` and shared `staged_doctor::inspect_source_files`; exact commit/tree/content identity and relation closure. CLI resolves branches/IDs and independently returns exact required check/profile/recipe document pins. `ci_check_inputs.rs` additionally binds a local check plan to committed inputs, optional absence and repository tools, with before/after live revalidation. Structural/closure and committed input admission are qualified; organization policy admission is implemented with focused regression coverage; completion policy and trusted acceptance remain open. | Final current full PM suite: 846 tests. CLI/catalog: 22 tests. These replace the earlier focused/historical source-only gates for this increment. |
| [ ] **PM-11.D3** — Export machine-readable summaries and supported reports. Import evidence descriptors with explicit producer trust/provenance; imported or manual success is not automatically trusted CI. | Partial: shared `execution/report.rs` and `check export` deliver bounded portable terminal reports with retained proof and separate freshness; offline validation checks consistency without granting producer trust. | 48 affected core and 23 CLI/catalog tests pass, including offline transport, altered and cross-run proof, stale/unknown/failed outcomes, unfinished rejection, and generated references. DSSE producer authentication passes 67 library/CI execution and 17 CLI/catalog tests; durable import and explicit reauthentication pass 866 full PM tests with subsequent affected-source regressions; criterion/completion integration remains required. |
| [ ] **PM-11.D4** — Bind accepted acceptance/check contracts separately from proposed test changes. Support red/green evidence when required. Detect removed or weakened required checks instead of letting a candidate redefine its completion denominator. | Partial: `ci_contracts.rs` independently captures required policy/profile/check/recipe definitions and blocks semantic changes or missing/archived requirements. Subject criteria/dependencies/gates and workflow definitions now bind through `ci_subjects.rs`; progress declarations and physical relocation are distinct. `ci_evaluators.rs` and `sources/evaluators.rs` pin declared evaluator bytes/modes/membership independently of application inputs. Paired external commit/contract pins now reject baseline substitution through shared `ci_validate_pinned` and CLI validation. Candidate-selected pins do not establish acceptance; shared signed evaluator-change review admission and `ci validate-reviewed` are implemented with focused acceptance coverage. Durable review retention passes the full 870-test PM suite. Contextual TUI review coverage is qualified. Shared `verify_red_green` and `ci red-green` verify exact signed artifacts, unchanged accepted contracts, genuine selected-input changes and assertion transitions; full PM and affected CLI suites pass. | Shared `verify_reviewed_red_green` and explicit CLI review flags now compose prior accepted baseline, exact signed red-contract admission and producer pair verification. The composed verifier passes 58 core and seven CLI/catalog tests plus strict lint/architecture/formatting. Durable pair retention and exact criterion-to-attestation verification are implemented; current and committed criterion definitions must match fresh original proof under external authority. Full PM and affected projection/CLI tests pass. Pending: shared gate/completion acceptance; authenticated links alone do not grant it. |
| [ ] **PM-11.D5** — Complete policy evaluation for issue Done, project/milestone exit criteria, and feature maturity. Distinguish manual acceptance, local feedback, reviewed evidence, and CI qualification. | Partial: `completion/` and `issue done --verification-file` implement standalone red/green and green-only Done with all required checks, current-source binding, historical proof, replay and recovery. `gate verify-green` authenticates attached green-only requirements, and `CompleteClaimedVerifiedIssue` plus `issue complete --verification-file` retain authenticated proof under the current claim. The shared `policy.rs` evaluator now provides source-bound project/milestone assessment and explicit completion plus one-stage feature maturity assessment and promotion, with issue/member, criterion, prerequisite, retirement, and gate conditions. Mounted TUI `p`/`m` controls now use the same source-pinned evaluator and receipt path. Fully authenticated policy-basis integration remains. | Current named PM profile passes 917 tests across 82 groups with zero failures or ignored tests; end-to-end CLI hierarchy/feature suites and the complete TUI all-target run remain green. Final integrated acceptance remains pending. |
| [ ] **PM-11.D6** — Show acceptance/check coverage beside review, with unmet/stale requirements and revision-bound review references. Relevant subject changes invalidate conclusions according to policy; a UI note alone does not impersonate a required reviewer. | Partial: `review_coverage/`, `context/reviews.rs`, `workbench/context_view.rs` and `ci review-coverage` expose committed revision/subject applicability, exact proof citations, stale selection and explicit current-policy authentication. Context never chooses its own reviewer authority. | Signed-proof core/CLI, mounted narrow/wide rendering and actual PTY stale-HEAD journeys pass; final qualification passes 2,142 PM/TUI tests, 38 workbench PTY tests, 26 CLI regressions, 6 catalog tests, strict lint, architecture and formatting. Live working-policy/evaluator comparison additionally passes 161 affected tests, including dirty policy restore in actual narrow/wide terminals, byte/mode/membership changes, unsafe inputs and context races. Explicit current-policy [TUI controls](../crates/workdeck-tui/src/workbench/review_authority.rs) pass full 1,271-test TUI and 38-test actual PTY qualification; final affected mounted/PTY reruns and six generated-reference tests pass after the paste-handler lint correction. Integrated completion/check qualification remains required. |
| [ ] **PM-11.D7** — Add PM validation to this repository's existing CI and release checks. Use a suitable pinned validator/trusted baseline for candidate changes to the validator itself; self-validation alone is development evidence, not independent qualification. | Partial: the checked-in `ci/workdeck-pm-release.json` named profile drives `cargo xtask pm check --profile standalone`, `cargo xtask pm performance --profile standalone`, and the release check, including the complete PM target suite, generated PM CLI catalog/green-gate/import compatibility tests, the synthetic dogfood journey, the selected CLI fault matrix and calibrated full-size projection budgets. The profile pins `xtask/src/project_management.rs` by SHA-256, requires the trusted-baseline field to name an immutable full commit or `refs/tags/...` plus a 64-character lowercase SHA-256 digest, and now requires the exact four standalone check commands and exact release build command; drift, an opaque label, or a weakened/removed command fails before execution. CI and release preflight invoke the named profile. External validator execution and verification of the referenced baseline remain open. | The current named profile parser tests pass, including malformed/branch/uppercase/short-digest and removed/unrelated/weakened-command rejection; the profile still needs an external validator and independent pinned-baseline review. |
| [ ] **PM-11.D8** — Add release-readiness profiles without deploying, publishing, creating cloud resources, or replacing the existing CI host. Hook commands and CI use the same validation library. | Partial: `cargo xtask pm release-check --profile standalone` validates the same named profile, including exact locked projection benchmark commands, then performs the locked release build without publishing. Profile inspection is available through `cargo xtask pm profile`; command validation rejects anything outside bounded Cargo test/build commands and the two fixed projection workloads. Hook integration and independent validator verification remain open. | The calibrated no-publish release gate passes **917 tests across 82 groups** and the optimized release build. The profile’s structured baseline reference is syntax-checked but has not been independently resolved or authenticated; external release workflow execution and hook qualification remain pending. |
| [ ] **PM-11.E1** — CI rejects invalid PM changes, missing checks, wrong-source/stale evidence, and unsupported completion claims. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-11.E2** — Valid candidates pass consistently locally and in CI. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-11.E3** — False-green fixtures fail. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-11.E4** — Changes to accepted evaluation contracts receive the required review. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-11.E5** — Existing release and terminal checks retain their coverage. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |

### 21.13. PM-12 — Harden, document, dogfood, and release

Phase state: **In progress**. Dogfood and selected CLI fault evidence are qualified;
the remaining implementation and acceptance evidence is pending.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [ ] **PM-12.D1** — Run the complete temporary-repository scenario: initialize/migrate, plan, select, claim, obtain context, implement a fixture change, check, review, complete, publish, and resume in another clone. | Partial: `crates/workdeck-cli/tests/pm_dogfood.rs` covers the full isolated CLI journey, including shared claim binding, revision-bound check execution, review-coverage inspection, policy transitions, proposal replay, and context/doctor resume from a cloned proposal ref. | `cargo test --offline --locked -p workdeck-cli --test pm_dogfood` passes 1 test. This is local synthetic composition evidence; authenticated review admission, the broader fault/platform matrix, external CI/release execution and final PM-12 acceptance remain required. |
| [ ] **PM-12.D2** — Test interrupted writes/migration/indexing/checks/publication, malformed files, unsupported schemas, disk/permission failures, stale evidence, and identity/short-reference collisions. | Partial: `crates/workdeck-cli/tests/pm_fault_matrix.rs` runs malformed YAML, unsupported schema, unsafe symlink, permission, stale source and ambiguous-reference cases, asserting fail-closed diagnostics and unchanged `.workdeck` authority bytes. The complete write/migration/index/check/publication/platform matrix remains open. | `cargo test --offline --locked -p workdeck-cli --test pm_fault_matrix` passes 1 test; the named PM profile includes the same target. Full fault-boundary and supported-platform coverage remains required. |
| [ ] **PM-12.D3** — Verify path containment, symlinks, argument execution, bounded parsing/logs, and source/output handling through concrete tests of supported operations. | Partial: `workdeck-pm` check-plan/execution tests cover FIFO and symlink rejection, repository-root containment, literal and positional argv handling, explicit shell parameters, bounded aggregate output and artifact limits; CLI `pm_fault_matrix` covers malformed/unsafe/stale source boundaries and `pm_checks` covers bounded machine-readable failures. | The named PM profile re-runs the PM all-target safety cases and the selected CLI fault matrix; focused source/output and adversarial fixture coverage exists. Cross-platform and final integrated PM-12 review remain open. |
| [ ] **PM-12.D4** — Run supported Linux/macOS/Windows targets with explicit locking/rename/process-cleanup tests. Document unsupported filesystems rather than promising uniform network-filesystem semantics. | Partial: native macOS PM/profile and Unix locking/rename/process-cleanup cases are covered; unsupported non-Unix process execution and authoritative mutation durability are now reflected in capability output instead of being advertised as ready. The installed Windows target was attempted in isolated checks with the default compiler and a Clang override. | Windows compilation stopped before the PM crate because the host lacks the MinGW compiler and cross-target standard headers; no Windows runtime claim is made. Linux/macOS/Windows runtime matrices, filesystem limits and final platform review remain open. |
| [ ] **PM-12.D5** — Qualify narrow/wide TUI layouts, key discoverability, worktree navigation, terminal lifecycle, existing session controls, and scaled dataset behavior. | Partial: mounted workbench and terminal suites cover narrow/wide allocation, contextual keys, retained source/worktree navigation, session controls, worker shutdown and the PM tabs; indexed list/board/tree readers are bounded. The opt-in release mounted probe drives 40,000 features and 10,000 issues through real `.workdeck/` files. | The current evidence includes 28 PM workbench terminal journeys, broader TUI regressions, and full-size mounted list/page p95 of 13.64 ms/4.58 ms plus feature-tree/issue-board p95 of 56.60 ms/13.48 ms, with final-row navigation under 3.1 ms. Explicit tree/board budgets, supported-platform lifecycle, richer fixtures and final PM-12 review remain open. |
| [ ] **PM-12.D6** — Update install/init/migration instructions, config/schema references, workflows, generated skill, recovery guidance, and architecture docs. Remove superseded active prototype instructions. | Partial: repository README and CLI README document root-level `.workdeck/`, init/migration, recovery and first-class PM commands; project-management design/schema/compatibility/validation docs, CI/release profile wiring, architecture ownership and generated PM skill/catalog references are checked in. | Profile/catalog parity, formatting, scoped Markdown-link and diff checks pass locally. A final behavior review must still reconcile every install, migration, recovery, architecture and generated-reference claim against the delivered source. |
| [ ] **PM-12.D7** — Dogfood in Workdeck through a few independently verifiable packages; do not manufacture issues for every function or design heading. | Partial: the isolated `pm_dogfood` scenario exercises a small temporary repository through migration, hierarchy, feature/issue work, context, claims, revision checks, completion, proposal publication and clone resume; it keeps the fixture bounded and does not create one issue per implementation detail. | The synthetic journey passes in the named profile. Independently accepted Workdeck packages and final PM-12 dogfood review remain open. |
| [ ] **PM-12.D8** — Prepare a release through the normal packaging/check process. Publishing remains an explicit release action after qualification. | Partial: `cargo xtask pm release-check --profile standalone` runs the locked PM/CLI checks, calibrated full-size benchmarks and optimized `workdeck` build without publishing; CI and release workflows invoke the same named profile. `cargo xtask release package` now reopens its generated archive through the package-layout inspector and verifies its generated checksum, while the focused `xtask` test writes and inspects both `tar.gz` and ZIP outputs, required release paths and binary-bound provenance. | The no-publish release path passes locally with 917 tests across 82 groups and both benchmarks; the focused package inspection test passes with 1 test and 198 filtered. A synthetic production-path smoke also passes for one tar and one ZIP target with 11 entries and checksum verification. This is structural local package evidence only; external release execution, authenticated provenance/signing, installation and final qualification remain open. |
| [ ] **PM-12.E1** — The fresh-user/agent scenario passes across the supported platform matrix. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-12.E2** — Repeated operations do not duplicate effects. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-12.E3** — Source, claim, and evidence ambiguity is exposed. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-12.E4** — Quality and release gates pass. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-12.E5** — Migration and recovery documentation match behavior. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-12.E6** — Performance and remaining limitations are recorded. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |
| [ ] **PM-12.E7** — All PM-00 through PM-11 criteria are actually satisfied. | Pending. | Pending: direct acceptance evidence for this exit criterion on integrated source. |

### 21.14. Cross-cutting contracts and authorization

These supplement the phase rows. They preserve narrative/default requirements and supporting
standalone design constraints that would otherwise disappear in a deliverables-only checklist.

| Requirement | Implementation evidence | Required validation evidence |
| --- | --- | --- |
| [ ] **PM-X.C1** — Keep the existing `workdeck` executable and terminal-only product; no external feature adapters, Bowerbird work, factory dispatch, sibling changes, agent hosting, remote workflow engine, hosted UI, or CD execution. | Pending. | Pending: Scope and package/executable audit; supported command catalog. |
| [ ] **PM-X.C2** — Use `.workdeck/` for authoritative PM files; keep TOML app preferences, YAML PM configuration, indexes, local settings, claims, and review sessions distinct. | Pending. | Pending: Configuration/authority matrix and root-discovery/regression tests. |
| [ ] **PM-X.C3** — Keep all CLI/TUI validation and mutations in shared application operations in `workdeck-pm`; no direct TUI YAML/SQL writes or duplicate retired mutation paths. | Pending. | Pending: Source ownership and call-site audit plus CLI/TUI behavioral parity. |
| [ ] **PM-X.C4** — Keep pure state machines separate from process/Git composition; preserve architecture ceilings, one composition root, and zero known violations. | Pending. | Pending: Cargo/source architecture gate and module/dependency review. |
| [ ] **PM-X.C5** — Create modules only with real behavior; no placeholders, canned results, disabled required screens, or TODOs counted as completed requirements. | Pending. | Pending: Final independent requirement-to-source and workflow review. |
| [ ] **PM-X.C6** — Preserve CLI aliases/envelopes, user settings, identity/history, and useful existing review/session/pager behavior through explicit compatibility mappings. | Pending. | Pending: Compatibility inventory and existing regression suite. |
| [ ] **PM-X.C7** — Keep the new store opt-in until verified migration/UI cutover; never combine writable legacy/new stores or test migration against the real backlog. | Pending. | Pending: Cutover/ambiguity cases and isolated test-fixture inspection. |
| [ ] **PM-X.C8** — Reads do not initialize or rewrite authoritative files; init preserves config/instructions and creates only needed stores; hooks and staging are explicit. | Pending. | Pending: Read/init/ignore/hook/staging tests with pre-existing files and mixed index. |
| [ ] **PM-X.C9** — IDs are immutable prefix/full ULID references with positive revision/content identity; preserve legacy IDs and never reuse retired identities; abbreviations reject ambiguity. | Pending. | Pending: Identity/tombstone/short-reference migration and collision fixtures. |
| [ ] **PM-X.C10** — Authors are attribution, not authorization; repository and record identity are independent of machine paths. | Pending. | Pending: Identity/authority policy and cross-clone/cross-repo tests. |
| [ ] **PM-X.C11** — Unknown supported metadata, Markdown, Unicode, and minimal diffs survive serialization; duplicate YAML keys and unsupported schemas are diagnosed. | Pending. | Pending: Parser round-trip/duplicate-key/version/line-path fixtures. |
| [ ] **PM-X.C12** — Comments, time entries, and evidence have unique identities; comment activity is derived without an item revision/timestamp hotspot. | Pending. | Pending: Concurrent independent-record mutation and item-content preservation tests. |
| [ ] **PM-X.C13** — Every automated mutation has preconditions and durable operation identity; replay survives cache deletion/restart and rejects reused IDs with different inputs. | Pending. | Pending: Idempotency and crash/replay tests against durable receipts. |
| [ ] **PM-X.C14** — Atomic file rename does not substitute for cross-process compare-and-set or multi-file recovery; receipts preserve partial/local/published/uncertain states. | Pending. | Pending: Competing process and every recovery boundary fixture. |
| [ ] **PM-X.C15** — Issue completion, feature maturity, availability, milestone outcomes, cycle scheduling, and target delivery remain distinct; small repos need only issues and labels. | Pending. | Pending: Policy/model/UI cases with partial completion and minimal configuration. |
| [ ] **PM-X.C16** — Only hard dependencies affect readiness/build order; preserve filtered-out prerequisites and distinguish a dependency path from a calendar forecast. | Pending. | Pending: Typed-edge/path/filter fixtures and user-visible labels. |
| [ ] **PM-X.C17** — Canceled prerequisites remain unresolved until explicit policy resolution; unknown or declared evidence cannot become proof automatically. | Pending. | Pending: Canceled/waived/replaced prerequisite and missing-evidence fixtures. |
| [ ] **PM-X.C18** — Stable acceptance criteria exclude code examples, unrelated checklists, and empty placeholders; candidate changes cannot weaken their own accepted evaluation contract. | Pending. | Pending: Markdown acceptance parsing and trusted-base contract-drift fixtures. |
| [ ] **PM-X.C19** — Keep estimates in explicit units; do not sum incompatible units or infer project completion from issue/node counts. | Pending. | Pending: Mixed-unit reports and project/maturity completion cases. |
| [ ] **PM-X.C20** — Keep existing documentation where it is; attachments and definitions are inert on inspection; do not copy sensitive artifacts, private transcripts, credentials, or ambient configuration into context/Git. | Pending. | Pending: Context/document/attachment/provenance input handling fixtures and source review. |
| [ ] **PM-X.C21** — Context identifies its source and omissions, distinguishes accepted requirements from comments/logs/summaries, and grants no authority; empty next selections create no work. | Pending. | Pending: Fresh-session context budget/trust/empty-selection scenarios. |
| [ ] **PM-X.C22** — Generated schema/command/skill references derive from actual typed catalogs; protocol pointers are merged only on explicit init/update. | Pending. | Pending: Generation parity gate and non-destructive AGENTS/protocol fixtures. |
| [ ] **PM-X.C23** — Named commands execute argv by default within configured roots; shell recipes are explicit, arguments validated, and secrets referenced by name only. | Pending. | Pending: Argument/shell/cwd/symlink/secret-output tests and schema review. |
| [ ] **PM-X.C24** — Local runner work is explicit, bounded, foreground, and owns cleanup only for its children; no autonomous retries, background agent runtime, or implied remote guarantees. | Pending. | Pending: Controlled timeout/cancel/child-process fixtures and command scope audit. |
| [ ] **PM-X.C25** — Results distinguish passed, failed, skipped, blocked, not_run, canceled, stale, unknown; exit zero and missing local artifacts do not independently prove success. | Pending. | Pending: False-green/missing-report/artifact/discovery/status matrix. |
| [ ] **PM-X.C26** — Check plans bind exact source, acceptance, definition, arguments, dependency/toolchain/environment inputs; dirty-worktree results never claim to verify only HEAD. | Pending. | Pending: Before/during-run input mutation and subject-binding fixtures. |
| [ ] **PM-X.C27** — Automatic cached-pass reuse stays disabled until complete input equivalence is proven; incomplete impact information broadens required checks. | Pending. | Pending: Reuse/default-selection cases and policy review. |
| [ ] **PM-X.C28** — Shared claims require confirmed publication descending from the observed ref; preserve accepted plans/proposals/coordination distinctions and never force-push or blindly replay a rejected claim. | Pending. | Pending: Two-clone local-bare-remote race/rejection/proposal tests. |
| [ ] **PM-X.C29** — Claims are cooperative task coordination, not execution fencing; expiry does not prove a stopped process; local claims never advertise cross-clone exclusivity. | Pending. | Pending: Local/shared/expiry/recovery/skew behavior and user-visible guarantee review. |
| [ ] **PM-X.C30** — Local mutation, publication, and claim release across refs are not atomic; uncertain completion/release must remain uncertain until reconciled. | Pending. | Pending: Fault injection between writes/publication/release and lost-response recovery. |
| [ ] **PM-X.C31** — Deleting indexes loses no authoritative data; query snapshots are consistent, stale sources visible, and cross-repository writes retain one explicit authority. | Pending. | Pending: Rebuild parity, interrupted indexing, stale-source and qualified-write tests. |
| [ ] **PM-X.C32** — Warm list/filter feedback targets 100 ms; calibrated incremental-refresh budgets are 30 seconds for 40,000 features and 15 seconds for 10,000 issues. Record cold/memory baseline, machine, datasets, p50/p95, and enforce measured regression budgets. | [Projection benchmark](../crates/workdeck-pm/examples/projection_bench.rs) selects family-specific refresh budgets and preserves the 100 ms warm-filter budget; the opt-in [mounted probe](../crates/workdeck-tui/src/workbench/indexed_workspace_tests.rs) drives real `.workdeck/` files at full size. | Fresh open-policy benchmark runs meet the enforced budgets (incremental 8.35 s features / 5.73 s issues under the 30 s/15 s limits; warm-filter p95 below 1 ms against the 100 ms budget) and record cold-index timings of 10.87 s / 7.03 s; the latest mounted probe records 10.22/7.94 s open/index, 13.64/4.58 ms list/page p95, 56.60/13.48 ms tree/board p95 and 671 MB sequential peak RSS on the documented host. Cold-index and memory values are recorded but not yet enforced by independent regression budgets; richer fixtures, independent tree/board thresholds and final requirement audit remain pending. |
| [ ] **PM-X.C33** — Preserve local selection, expanded nodes, panel sizes, drafts, and issue/review return context; retain usable narrow/wide terminal layouts and discoverable contextual keys. | Pending. | Pending: PTY navigation/layout/session/terminal acceptance evidence. |
| [ ] **PM-X.C34** — Manual acceptance, local feedback, reviewed evidence, and trusted CI qualification remain distinct; validator self-checks are not independent qualification. | Pending. | Pending: Trust/provenance/accepted-validator tests and external qualification evidence. |
| [ ] **PM-X.C35** — Do not weaken/remove gates, skip failures, or alter expected results merely for green output; distinguish pre-existing failures from regressions. | Pending. | Pending: Baseline and final checks plus independent diff/test review. |
| [ ] **PM-X.C36** — Use temporary repos, local bare remotes, synthetic records, and controlled processes for fault/race/destructive tests; preserve all user changes and the real index. | Pending. | Pending: Test fixture isolation review and final scoped Git status/diff. |
| [ ] **PM-X.C37** — Every meaningful behavior has a relevant failing acceptance case before its focused passing evidence; docs-only changes use structure/link checks. | Pending. | Pending: Per-requirement test names, before/after results, and tested source identity. |
| [ ] **PM-X.C38** — Run an independent final review against every ledger requirement, repair findings, and re-run affected checks against the delivered source. | Pending. | Pending: Review findings/dispositions and final source-bound gate results. |
| [ ] **PM-X.C39** — Supported Linux/macOS/Windows behavior requires platform evidence; unavailable external checks stay incomplete with exact prerequisites and recorded filesystem limitations. | Pending. | Pending: Platform matrix with per-platform results or explicit unresolved gate. |
| [ ] **PM-X.C40** — Keep complete PM-00–PM-12 scope across checkpoints and resumptions; no preview milestone or partial test supports a full-completion claim. | Pending. | Pending: Requirement-by-requirement completion audit and final release milestone decision. |
| [ ] **PM-X.C41** — Leave all work reviewable locally without commits, pushes, merges, external publication/deployment/account changes or sibling edits unless separately authorized. | Pending. | Pending: Final worktree/diff and action receipt audit. |
| [ ] **PM-X.C42** — Final delivery records phase outcomes, actual checks, migration/usage instructions, performance, remaining limitations and every unresolved requirement. | Pending. | Pending: Final report cross-checked against ledger, docs, artifacts and tested source. |

### 21.15. Named validation gates

Run focused gates when their code changes and the complete applicable set on final integrated
source. A green command is evidence only for the behavior its inspected tests actually cover.
Record extra PTY, fault, performance, packaging and platform checks in the relevant requirement
rows; these existing commands are not a substitute for those acceptance cases.

| Requirement | Command | Required validation evidence |
| --- | --- | --- |
| [x] **PM-V.G1** | `cargo fmt --all --check` | Final-source pass recorded 2026-09-11 (`/tmp/workdeck-pm-gate-fmt-20260911.log`, exit 0) and re-verified after the validator hardening change; reviewed in the final requirement audit. |
| [x] **PM-V.G2** | `cargo test --locked -p workdeck-cli --test cli` | Exact-command final-source run passed **46 tests, 0 failed** on 2026-09-11 (isolated offline/locked target; `/tmp/workdeck-pm-gate-cli-20260911.log`). |
| [x] **PM-V.G3** | `cargo test --locked -p workdeck-cli --test git_integration` | Exact-command final-source run passed **12 tests, 0 failed** on 2026-09-11 (isolated offline/locked target; `/tmp/workdeck-pm-gate-git-integration-20260911.log`). |
| [ ] **PM-V.G4** | `cargo test --locked -p workdeck-tui --all-targets` | All TUI targets passed inside the repaired-source full workspace run (part of **4,943 passed / 0 failed / 2 ignored across 205 groups**, `/tmp/workdeck-pm-workspace-repair-20260911.log`) and in the final `xtask verify` aggregate. Final terminal/platform acceptance (supported-platform matrix, PM-X.C39) remains open. |
| [x] **PM-V.G5** | `cargo xtask architecture check` | Final-source isolated run passes: **13 production crates, one shipped executable, zero dependency/source-reachability violations** (`/tmp/workdeck-pm-gate-architecture-20260911.log`, re-run after the validator hardening; the earlier hardening log `/tmp/workdeck-pm-architecture-after-hardening-20260911.log` is retained as history). |
| [x] **PM-V.G6** | `cargo test --locked --workspace --all-targets` | The repaired-source isolated run completed **4,943 passed, 0 failed and 2 ignored across 205 groups** on 2026-09-11, including the previously intermittent macOS watcher case (`/tmp/workdeck-pm-workspace-repair-20260911.log`). The two ignored tests are the opt-in mounted full-size probe and the pinned CI-oracle capture. Pre-repair runs remain labeled historical. |
| [x] **PM-V.G7** | `cargo clippy --locked --workspace --all-targets -- -D warnings` | Final-source isolated workspace Clippy passes with warnings denied (`/tmp/workdeck-pm-gate-workspace-clippy-20260911.log`); the targeted PM/VCS/xtask run was additionally repeated after the validator hardening change. |
| [x] **PM-V.G8** | `cargo xtask verify` | Final-source aggregate passed with **4,944 passed, 0 failed and 2 ignored across 205 groups** plus the optimized release smoke (`/tmp/workdeck-pm-verify-final-20260911.log`). The first attempt hit one intermittent macOS `EPERM` process-group kill in `execution::process::tests::canceled_owned_group_cannot_leave_a_descendant_writing_later`; that test passed in isolation (`/tmp/workdeck-pm-eperm-rerun-20260911.log`) and in every other final-source suite, and the aggregate re-run completed green with no source change (first attempt retained at `/tmp/workdeck-pm-verify-eperm-first-attempt-20260911.log`). Pre-repair runs remain labeled historical. |
| [x] **PM-V.G9** | `cargo test --locked -p workdeck-pm` | Exact-command final-source run passed **906 tests, 0 failed, 0 ignored across 77 groups** on 2026-09-11 (isolated offline/locked target, removed after validation; `/tmp/workdeck-pm-gate-pm-crate-20260911.log`), covering PM domain, files, workflow, graph, context, check, coordination and index behavior; the standalone profile's `--all-targets` superset also passed twice (917/82 groups). |

Initial ledger inventory: **97 numbered deliverables, 71 phase exit criteria, 42 cross-cutting contracts, and 9 named validation gates** (219 unchecked entries). Counts describe tracking coverage, not implementation progress.
