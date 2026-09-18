# workdeck-cli

`workdeck` is a terminal repository and review workbench. It combines continuous
diff review with files, Git history, Markdown-backed issues, imported session
annotations, and search. Planning remains accessible in clean and dirty repositories;
Workdeck does not host coding-agent processes.

```sh
cargo install --path crates/workdeck-cli
workdeck
```

Initialize planning files under repository-root `.workdeck/`:

```sh
workdeck init
workdeck issue create "Review the change" --json
```

`workdeck --init` remains an alias. Press F3 for Issues and F2 to return to Review;
F4 creates an issue from the selected review file or note. Application preferences
remain TOML in `.workdeck/config.toml`; PM configuration is `.workdeck/config.yml`.

In Issues, press `w` to switch between the list and board. On the board, `z`
cycles status, priority, assignee, project, cycle and milestone grouping;
Left/Right selects a column and Up/Down selects cards. `/` keeps the shared
filter and sorting controls. `Enter` opens the selected source; Shift-PgUp/PgDn
scrolls that retained document. Existing `e` edit and `s` status forms apply to
the selected issue and preserve the usual source checks. Board columns read
bounded windows from one captured query; refresh and source errors remain visible.

In Features (`v` from Issues), `/` opens filters for text, project, milestone,
target, lead, decision, maturity and availability. Blank fields match all; Ctrl-S
applies the filter and Esc cancels. Filtering preserves unsaved feature drafts.
`t` switches the list to a native feature tree.
Left collapses an expanded branch or moves to its parent; Right expands a branch
or enters its first child. The tree preserves selected feature identities across
refresh and tab changes. Collapsed branches affect only the read view. A parent
outside the query is labeled explicitly; feature edits still use native source
checks and do not infer implementation or qualification from tree position.

Cycle carryover uses a reviewed, recoverable batch:

```sh
workdeck cycle carryover previous next --json --no-input
workdeck cycle carryover previous next --expected-preview HASH --request-id REQUEST --json --no-input
```

The preview lists unfinished members and explains completed/canceled/archived/retired
exclusions. Repeat `--issue ID` to select an explicit eligible batch (maximum 100).
Apply uses the same issue mutation rules and requires unchanged membership, cycle
records and validation inputs. It changes cycle membership only. Retry an uncertain
apply with the original arguments and request ID; recover interrupted operations with
`workdeck operation recover`. `--stage` applies only to the mutation's exact files.
In Cycles (F12), `u` opens carryover for the selected cycle. Enter the destination
cycle ID and optional issue IDs, then Ctrl-S previews the batch. Review moves and
exclusions before pressing `x` to apply. PgUp/PgDn scroll the preview; Esc retains
it across tab changes. After an attempt, `x` retries the original plan and request.
Ctrl-D explicitly discards the retained intent so a fresh plan can be reviewed.
Interrupted transactions show `workdeck operation recover` guidance; after recovery,
retry the original request. Carryover keeps the source cycle token captured when
the form opened and never silently rebases a stale preview.

Shift-F12 opens Activity from any workbench tab. It shows native event records
newest first, with record kind and timestamp; unknown times are labeled. Up/Down,
PgUp/PgDn and Home/End navigate bounded windows. Enter opens the exact event source;
Shift-PgUp/PgDn or the mouse wheel over the source scrolls the retained excerpt.
`r` refreshes the timeline while preserving the opened source generation. F2 returns
to Review and F3/Esc to Issues; issue drafts and the timeline's selection remain
retained across tab changes. Source errors keep last-good results visibly stale.
This working-tree timeline is a read view, not completion evidence. The indexed CLI
also supports typed activity queries by subject, record kinds and time interval.

Shift-F9 opens My work across explicitly registered checkouts. `1` shows assignments,
`2` shows review requests, `3` overdue assignments, `4` blocked work, and `5` active
claims for the workbench author
(`local` by default). `/` changes the actor and optional checkout aliases; Ctrl-S
applies the filter. Review requests match the reviewer in a Review-category workflow
state. Overdue excludes completed/canceled work; date-only deadlines remain due
through their UTC calendar day. Timestamp deadlines retain offsets and nanoseconds.
Refreshing captures a new evaluation time; paging keeps the inspected time. Registration uses
`workdeck repository inspect` and the explicit `workdeck repository register`
request. Reading an empty registry creates no mappings.

Up/Down and PgUp/PgDn move within a bounded report page; `[` and `]` move between
pages of the same captured generation. The last 128 previous pages are retained;
`r` explicitly restarts from page one with a fresh report. Changed generations
reject page navigation and preserve the inspected page. `s` toggles the registered
source list, including unavailable/stale sources and their exact mappings. Known
match totals are labeled incomplete when sources are unavailable.

`e` toggles typed work evidence: prerequisite and question blockers, or claim
assessment, observation scope and whether selected requirements still match. Shared
claims remain unconfirmed observations. Claim actor is independent of assignment;
expired active claims stay visible for recovery, while released claims are excluded.

Enter opens the exact cached source excerpt for a selected work item. Shift-PgUp/
PgDn or the mouse wheel scrolls that excerpt. Refresh, filter and tab changes keep
the opened bytes and source identity; a replaced mapping cannot redirect them.
F2 returns to Review; F3/Esc returns to Issues with the original issue draft intact.
These observations do not authorize cross-repository writes, confirm shared claims,
or satisfy completion evidence.

In My work, `o` opens the selected assignment's or source row's registered source.
`b` returns to the previous retained source; `h` returns directly to the original
launch checkout. Each checkout/source context retains its
planning forms, selection, filters, open file review, command cwd and navigation
context. A persistent header shows the active checkout alias and root. A
switch reloads review/configuration and its command cwd through the selected
checkout's own provider; a failed load leaves the current source and drafts intact.
Returning to a copied/replaced source directory is rejected even if it has the same
repository ID. Existing check/claim/publication workers stay bound to their source,
share foreground-run ownership, and are polled and joined while inactive.

A session retains at most eight checkout/source contexts, including the active one.
Reaching that bound preserves all existing contexts and refuses a ninth; another
Workdeck session can open additional checkouts. The launch registry and its cached
excerpts follow the user without creating a registry in selected repositories.
Accepted/proposal mappings open read-only indexed Issues, Features, Planning and
Activity with exact excerpts, filters, boards and feature trees. They have no native
planning mutation controller. Newly opened tabs share the captured revision;
`r` explicitly refreshes that view. Retained excerpts keep their original bytes.
Code review still reads the selected physical working checkout. CLI overdue and
claimed reports require explicit `--as-of` timestamps, retained across pagination.

Indexed CLI reads use explicit cache refresh and source-bound query inputs. Save
this as `feature-query.json`:

```json
{"kind":"features","query":{"tree":true}}
```

```sh
workdeck index refresh --json
workdeck index query --input feature-query.json --json
workdeck schema projection-query --json
workdeck schema projection-limits --json
```

`refresh` updates only the disposable local index. `query`, `board`, and `show`
read an existing cache without creating, repairing, or refreshing it. Their
response envelopes retain the selected source, projection identity, and `cached`
freshness, including with `--fields` or `--compact`. Cached data does not validate
the current working tree or establish completion evidence.

For later query pages, pass the exact serialized `result.page.handle` through
`--expected-query` with `--offset`. Board inputs use an Issues query with an explicit
`group_by` such as `status` or `assignee`; `index board` returns bounded columns and
its handle. `index show --input token.json` reads the exact row token returned by a
query. Changed query/source/checkout handles fail rather than selecting a different
record. `--source` selects working-tree, accepted, proposal, or coordination;
proposal also requires `--reference` with the full ref name.

Native relationships and capability declarations are available in qualified PM-06:

```sh
workdeck issue relations ISSUE_ID --json
workdeck issue prerequisite ISSUE_ID add PREREQUISITE_ID --json
workdeck issue ready ISSUE_ID --json
workdeck issue dependency-path ISSUE_ID PREREQUISITE_ID --json
workdeck feature create "Search" --request-id feature-search-1 --json
workdeck feature coverage FEATURE_ID --json
workdeck gate list --json
workdeck evidence list --json
```

Use returned IDs in place of the uppercase placeholders. Press `b` from Issues to inspect
and traverse the graph, or `v` to open Features; `n` creates, `e` edits, and `r` refreshes.
Feature maturity is a separate declaration and does not advance when an issue finishes.
Declared evidence never establishes a verified gate result. Feature/gate `delete --dry-run`
previews permanent retirement and incoming blockers; deletion requires the full canonical ID
and `--yes`. Use `archive` for reversible archival. See `workdeck schema --json` and each
command's `--help` for typed authoring inputs and source preconditions.

PM-07 provides qualified agent context and continuity:

```sh
workdeck capabilities --json
workdeck context --issue ISSUE_ID --budget 16384 --json --no-input
workdeck next --issue ISSUE_ID --json --no-input
workdeck issue next --limit 10 --json --no-input
workdeck question list --json
workdeck handoff list --issue ISSUE_ID --json
workdeck skill path workdeck-pm
```

Press `i` from Issues for Context, then `1`–`5` for Context, Next, Questions,
Handoffs and Checks. Question and handoff drafts retain their inspected source and request
identity across F2/F3 navigation. Changed requirements make historical continuity
visibly stale; handoff prose and answers never become verified check outcomes.
See the [schema and continuity contract](../../docs/project-management-schema.md)
and [generated command reference](../../docs/reference/workdeck-pm-commands.md).

`workdeck protocol preview --json` shows a proposed thin pointer in root `AGENTS.md`.
Explicit `protocol install` requires `--expected-content HASH` from that preview
(or `--expect-absent`), `--expected-repository REPO_ID` and `--request-id ID`. Use `protocol update` for an existing
managed block. Surrounding instructions remain byte-for-byte intact. Pointer
receipts and recovery are machine-local, separate from canonical PM history;
retry an uncertain write using the same mode, request and original precondition.

Local command/check execution uses authored files in `.workdeck/commands/`,
`checks/` and `check-profiles/`:

```sh
workdeck command list --json
workdeck check profile list --json
workdeck check plan --profile quick --json --no-input
workdeck check run --plan PLAN_HASH --expected-plan PLAN_HASH --actor agent-name --request-id stable-run-id --json --no-input
workdeck check results --status failed,stale,unknown --json
workdeck check explain RUN_ID --json
workdeck check export RUN_ID --json > check-report.json
```

Planning is inert. Execution is explicit Unix foreground local feedback with
bounded output, owned-child cleanup and durable run receipts. Reuse the original
plan, actor and request after an uncertain response; `check recover RUN_ID` never
starts a replacement process. Missing artifacts or changed inputs invalidate the
current assessment. CI trust and required-check completion admission remain
separate later-phase work. See the [validation record](../../docs/project-management-validation.md)
for current phase qualification.

Validate committed planning and baseline evaluation contracts without using dirty
working files or the index:

```sh
workdeck ci validate --base BASE_COMMIT --head HEAD_COMMIT --json --no-input
```

Exact commit IDs, full refs, local branch names and `HEAD` resolve to reported
commit/tree/content identities. Both commits must contain planning for the same
repository. The report independently pins baseline and candidate required profiles,
checks, command recipes and acceptance/workflow policy. It also binds issue criteria
and dependency/gate/feature links, project exit criteria, milestone outcomes, feature
criteria/decision state and dependency/gate links, and gate requirement definitions. Missing or archived requirements fail;
semantic changes require review. YAML comments may change exact document fingerprints
without changing the semantic contract. Issue checkbox/prose edits and physical feature
relocation retain semantic identity; the exact document pins still change. Removing or
rewriting requirements, gate freshness/check pins or workflow semantics requires review.
Required CI checks must explicitly declare evaluator dependencies in their check definition:

```yaml
evaluator_inputs:
  files: [tools/assert-result.py]
  trees: [tests]
```

Selectors are literal repository-relative paths. Each must also be covered by the
command's mandatory inputs (`files`, dependency/toolchain files, or `trees` as
appropriate). CI captures committed bytes, executable modes and tree membership
independently in each revision; evaluator changes require review even when check
YAML is unchanged. Application-only edits leave the evaluator contract unchanged.
Missing inputs, symlinks and case-colliding paths fail. Git metadata and engine
output are excluded. An explicit `{}` declares no repository evaluator inputs;
omitting the field remains compatible with local checks but fails for required CI
checks. These declarations cannot prove arbitrary process access or sandbox isolation.
Validation never fetches or executes checks.
A caller-selected baseline is not automatically accepted/trusted, and successful
validation does not authorize completion. Review admission, CI execution/provenance
red/green evidence and complete completion/review policy remain open implementation requirements.

Prepare and run committed check feedback in a supplied CI checkout:

```sh
workdeck ci plan --revision HEAD --profile unit --json > ci-plan.json
# Inspect the plan and copy result.binding.fingerprint from ci-plan.json.
workdeck ci check --plan-file ci-plan.json --expected-plan BINDING_FINGERPRINT \
  --actor ci-runner --request-id stable-ci-request --json
```

The saved file can be the complete successful `ci plan` JSON output or a raw
`CiPreparedCheck`. Save it outside selected input trees. Planning also accepts
`--issue ID`; its source and inherited requirements must match the commit. All
selected checks must declare evaluator inputs. Required configured checks remain
selected alongside the requested profile. Dirty bytes, selected untracked members,
missing committed optional inputs and changed repository tools reject admission.
External tool and environment hashes remain bound in the local plan.

Execution retains the exact revision binding before spawn. Keep the same file,
fingerprint, actor and request ID for retries: they recover the original run without
another execution. `check status --help` and `check recover --help` describe retained
run inspection/recovery. Results include state and receipts; failed, stale, unknown,
blocked, skipped and canceled runs return nonzero. A successful command response
means it returned a run result; inspect `result.state` as well as the exit status.

Foreground execution currently requires Unix. These commands run recipes in the
supplied checkout and retain the
`local_feedback` basis. They do not allocate a sandbox, accept a baseline, approve
evaluator changes, confer producer trust or authorize completion. Use `ci validate`
for baseline/candidate planning comparison; integrated trust, red/green and completion
admission remain open requirements.

For an existing prototype backlog, review and apply an explicit migration:

```sh
workdeck migrate legacy --plan-out migration-plan.json --json
workdeck migrate legacy --apply --plan migration-plan.json --request-id migrate-backlog-1 --json
```

Migration preserves the old source. See the [schema and migration contract](../../docs/project-management-schema.md)
and [implementation plan](../../docs/project-management-implementation-plan.md) for current coverage
and remaining work.

Print a read-only JSON status snapshot:

```sh
workdeck --status-json
```

`check export RUN_ID --json` returns a portable `CheckReport` in `result`.
It retains the original intent, terminal result, reservation/publication receipts,
parsed report counts and failures, and a separate freshness observation timestamped by `observed_at`.
Exporting a failed or stale run succeeds as a read operation; inspect the report's
states before interpreting it. A run without a published terminal result cannot be
exported. Export does not execute or recover a run.

The shared `CheckReport::from_json` accepts the raw `result` object and validates
its bounded transport shape, record/receipt relationships and fingerprint without
reading the checkout. A valid fingerprint does not authenticate the attributed actor
or establish CI qualification. Logs and artifact bodies are referenced by hash and
are not embedded. Trusted producer admission and evidence import remain in progress.

Producer authentication is available independently of a local checkout:

```sh
workdeck ci policy --policy-file /trusted/workdeck-producers.json --json
workdeck ci authenticate --report-file signed-check-report.json \
  --policy-file /trusted/workdeck-producers.json \
  --expected-policy TRUSTED_POLICY_HASH --expected-commit EXACT_COMMIT_SHA --json
```

The policy is a `producer-trust-policy` JSON object with repository identity and
1–16 producers. Each producer has an ID, a base64 32-byte Ed25519 public key,
`not_before`/`expires_at` timestamps, and a `checks` map from check ID to exact check
definition hash. Obtain the expected policy fingerprint and commit from the
admitting environment independently of candidate files and the report. Inspecting
an arbitrary policy does not make it trusted. Workdeck does not discover a trust
policy from the candidate or handle private signing keys.

The signer wraps the raw `check export` result in a standard
[DSSE envelope](https://github.com/secure-systems-lab/dsse/blob/master/protocol.md),
using payload type `application/vnd.workdeck.check-report.v1+json` and an Ed25519
signature over DSSE pre-authentication encoding of the exact payload bytes. Standard
and URL-safe base64, with or without padding, are accepted. `keyid` is ignored for
authority; keys are verified against the pinned policy. Duplicate producer IDs or
public-key aliases are rejected. One currently valid producer must authorize every
check definition in the signed report; separate producers cannot combine partial
scopes. The source must have a retained committed-input binding matching the policy
repository and explicitly expected commit. Future observations are rejected.

Input limits are 512 KiB for policies, 96 MiB for signed envelopes, 64 MiB for decoded
reports, eight signatures, and a conservative 512 MiB aggregate payload-hashing work
budget across policy keys and signatures. Malformed signatures are rejected.

A successful `ci authenticate` means authentication succeeded, including when the
signed report records failed, stale or unknown checks. Its output preserves the
report's local-feedback basis and states. The authentication result is not a reusable
credential: downstream evidence admission must reverify the original envelope under
its independently selected policy. Accepted-baseline review, durable evidence import,
red/green and completion qualification remain in progress.

Durable report imports are available at the native repository root:

```sh
workdeck ci import-report --report-file signed-check-report.json \
  --policy-file /trusted/workdeck-producers.json \
  --expected-policy TRUSTED_POLICY_HASH --expected-commit EXACT_COMMIT_SHA \
  --actor ci-importer --request-id STABLE_IMPORT_REQUEST --json
workdeck ci reports --json
workdeck ci report ATST_ID --json
workdeck ci reauthenticate ATST_ID --policy-file /trusted/workdeck-producers.json \
  --expected-policy CURRENT_TRUSTED_POLICY_HASH --expected-commit EXACT_COMMIT_SHA --json
```

Imports retain immutable `.workdeck/attestations/ATST-<ULID>.json` records and original
operation receipts. Retain all original inputs when replaying a request; replay returns
its historical receipt and does not renew producer authentication. `ci reports` exposes historical summaries;
`ci report ID` returns the full retained proof; `ci reauthenticate` verifies retained signed
bytes under the explicitly supplied current policy. Missing or altered authority is
an error. Interrupted imports use `operation recover`; recovery never starts checks.
A failed report remains failed when stored or reauthenticated. These commands do not
satisfy issue Done or feature/project exit policies; criterion/completion integration
is still in progress.

Imported records are bounded to 8 MiB each and 256 records/64 MiB per catalog, within
the existing global operation-history and encoded transaction bounds. The smaller
storage bound is distinct from standalone authentication's transport limits. Oversize
imports fail before publication. Raw logs and artifact bodies remain separately
referenced; this store retains signed report descriptors and their provenance.


CI baseline pins can be supplied as a pair:

```sh
workdeck ci validate --base ACCEPTED_SHA --head CANDIDATE_SHA \
  --expected-base-commit ACCEPTED_SHA \
  --expected-base-contract ACCEPTED_CONTRACT_HASH --json
```

Obtain the accepted commit and contract fingerprint from an independent trusted
channel. Copying them from the candidate does not establish acceptance. The command
captures the committed sources afresh and rejects either mismatch. Matching pins
produce `pinned_baseline_validation`; candidate contract changes still require review,
and this result does not attest a reviewer or qualify successful check execution.
Without the pair, existing caller-selected planning validation remains compatible.


Authenticated review of evaluation-contract changes:

```sh
workdeck ci review-policy --policy-file reviewers.json --json
workdeck ci validate-reviewed --base ACCEPTED_SHA --head CANDIDATE_SHA \
  --expected-base-commit ACCEPTED_SHA --expected-base-contract ACCEPTED_HASH \
  --policy-file reviewers.json --expected-policy REVIEW_POLICY_HASH \
  --review-file signed-review.json --json
```

The external policy lists `required_reviewers` with unique IDs, Ed25519 public keys
and validity windows. Every listed reviewer must sign the same DSSE payload of type
`application/vnd.workdeck.contract-review.v1+json`. Private keys and signing remain
outside Workdeck. The `ci-contract-approval` schema defines the payload: exact baseline
pins, full candidate source identity, candidate contract hash, approval decision,
review time and expiration. Inspect those values in `ci validate` output (including
the structured failure report when a contract change requires review). Obtain baseline
and policy pins through an independent accepted channel. Policy inspection alone does
not establish trust.

The shared operation verifies all signatures and time windows, recaptures immutable
sources, and rejects changed candidates. `validation` retains the unreviewed report;
`admission` identifies authenticated reviewers, and top-level `valid` combines review
admission with structural and organization-policy validation. Successful review does
not establish passing checks, red/green evidence, or completion. Serialized admission
output is an observation, not a credential; future admission must reverify the original
envelope under current policy.


Retain and revalidate signed reviews:

```sh
workdeck ci import-review --review-file signed-review.json \
  --policy-file reviewers.json --expected-policy REVIEW_POLICY_HASH \
  --expected-base-commit ACCEPTED_SHA --expected-base-contract ACCEPTED_HASH \
  --expected-commit CANDIDATE_SHA --actor USER --request-id REQUEST_ID --json
workdeck ci reviews --json
workdeck ci review CRVW_ID --json
workdeck ci reauthenticate-review CRVW_ID \
  --policy-file reviewers.json --expected-policy CURRENT_REVIEW_POLICY_HASH \
  --expected-base-commit ACCEPTED_SHA --expected-base-contract ACCEPTED_HASH \
  --expected-commit CANDIDATE_SHA --json
```

Import retains exact envelope text and original pins in immutable
`.workdeck/contract-reviews/CRVW-<ULID>.json` records, backed by the normal operation
receipt/recovery engine. Reusing the request ID returns its original receipt without
renewing trust. List output contains summaries; reading by ID returns the full proof.
Doctor and native snapshots validate original signature/receipt consistency, including
missing or edited records. Historical reads and snapshot restoration work without
Git objects. Current reauthentication additionally requires the committed baseline
and candidate objects and independently selected current policy/pins.

Records are capped at 1 MiB, with at most 256 reviews and 16 MiB total retained review
content, subject to existing operation/journal limits. Activity displays historical
import events; an old approval does not become a current passing review or completion.


Review coverage for a committed revision and optional subject:

```sh
workdeck ci review-coverage --revision HEAD --working-tree --subject issue:ISSUE_ID --json
workdeck ci review-coverage --revision CANDIDATE_SHA --subject feature:FEATURE_ID \
  --policy-file reviewers.json --expected-policy REVIEW_POLICY_HASH \
  --expected-base-commit ACCEPTED_SHA --expected-base-contract ACCEPTED_HASH --json
```

Subjects accept `issue:ID`, `feature:ID`, `gate:ID`, or a planning kind such as
`project:ID`. Optional `--expected-subject HASH` requires the selected document's exact
bytes to match the committed subject. Without authority inputs, this is an informational
report: a historical match remains unauthenticated. With all external authority pins,
the command fails unless a retained review authenticates the selected revision/subject.
Fresh reviewer attribution is separate from historical signer names. No matching proof,
expired approval, wrong source, absent subject requirements and rejected policies remain
non-authenticated. This gate covers review of the contract, not successful checks or
criterion/completion acceptance.

Task context shows up to five retained review observations and proof citations. It
compares committed HEAD with the selected issue document, live planning contracts and
declared evaluator bytes/modes/directory membership, displays the assessed HEAD,
and rechecks the observation before publishing the context anchor. Explicit refresh
updates stale states; changing HEAD during capture rejects the packet. Historical rows
come after requirements, blockers and continuity in the packet budget. The workbench
does not select a current reviewer policy from candidate files. Use the explicitly
pinned CLI assessment for current review authentication. Add `--working-tree` to that
command to require the live planning/evaluator comparison too; without it, inspection
remains committed-only. Unavailable, unsafe or unsupported live captures are unknown and
cannot authenticate. The report exposes the observed working contract/evaluator hashes.
The comparison includes untracked evaluator tree members and exact planning document
bytes; even cosmetic edits may conservatively invalidate coverage. It does not certify
application files outside the evaluator selection.

In normal TUI startup, open an issue's task context and press `v` to authenticate
its retained reviews. Paste the independently obtained reviewer-policy JSON, its
expected hash, and accepted baseline commit/contract hashes. The form supplies no
candidate-derived defaults. `Ctrl-S` performs a read-only shared assessment of HEAD,
the selected issue and live planning/evaluator inputs.

The separate **Review authentication** row shows current reviewer identities,
assessment time, HEAD, policy pin and rejection/staleness reasons. `r` revalidates
the last submitted pins. Editing any input clears the previous assessment; malformed
inputs and failed refreshes cannot leave an authenticated result visible. `Esc`
retains the draft for that task; `Ctrl-D` inside the form discards draft and authority.
No policy, approval, check result or operation is written. Historical context/proof
citations remain separate, and this assessment does not grant completion.

Verify required red/green evidence for a committed check:

```sh
workdeck ci red-green --check unit --revision GREEN_SHA \
  --expected-base-commit ACCEPTED_RED_SHA --expected-base-contract ACCEPTED_RED_CONTRACT \
  --policy-file producers.json --expected-policy TRUSTED_PRODUCER_POLICY_HASH \
  --red-report-file red.signed.json --green-report-file green.signed.json \
  --red-artifact-file red.xml --green-artifact-file green.xml --json
```

The independently accepted, required JUnit check declares its assertion identities
and allowed red process exits, for example this fragment in `.workdeck/checks/unit.yml`:

```yaml
red_green:
  red_exit_codes: [1]
  cases:
    - suites: [unit]
      class_name: Behavior
      name: regression
```

Obtain the red baseline commit/contract pins through an independent accepted channel.
Proposed evaluator changes require separate review/admission before that revision can
be used as an accepted baseline; this command does not promote it. The candidate must
descend from the red baseline, with unchanged exact evaluation contracts. Producer
scopes must authorize the exact check definition, including `red_green`.

Run the existing `ci plan`/`ci check` workflow at red and green revisions. Export red
feedback and preserve its original XML **before** changing selected inputs, so the
exported observation is not stale. Green execution must begin after red finishes.
An admitted producer signs the raw `check export` result, not its CLI transport
wrapper. Preserve both original signed envelopes and exact UTF-8 XML artifacts for
reverification. The verifier never runs a check, changes a checkout or writes PM data.

The shared operation reauthenticates producer signatures and recaptures committed
input proofs. Arguments, environment and tools must remain comparable, and selected
committed inputs must change. Required cases must fail by assertion on red and pass
on green. Infrastructure errors, unsupported exits, missing/changed artifacts, stale
reports, replaced cases and newly skipped executed cases fail. Additional green cases
are permitted. Case identity is the full suite path, class name and test name, without
concatenation or truncation. XML ambiguities and inventory limits fail rather than
returning partial coverage. Limits are 4 MiB per XML document, 20,000 cases and 2 MiB
aggregate identity text; required contracts name 1–128 cases and 1–16 red exit codes.

Success returns `authenticated_check_pair`, exact source/provenance hashes, required
cases and changed selected inputs. It does not change the original local-feedback
results, grant issue completion or provide a reusable credential. Durable evidence
and criterion/completion integration remain in progress.


For a newly proposed red evaluator contract, add all five review options to the
`ci red-green` command above:

```sh
  --baseline-review-file red-contract-review.json \
  --review-policy-file reviewers.json \
  --expected-review-policy "$REVIEW_POLICY_HASH" \
  --accepted-commit "$PRIOR_ACCEPTED_COMMIT" \
  --accepted-contract "$PRIOR_ACCEPTED_CONTRACT"
```

The original review must approve the exact red commit and contract relative to the
independently accepted prior baseline. Obtain the reviewer policy and pins through
the accepted channel; do not derive trusted authority from candidate-owned files.
Every required reviewer must authenticate, the red commit must descend from prior
acceptance, and the red-to-green evaluator contract must remain unchanged. Output
kind `ci.red_green_reviewed` contains separate `review` admission and `pair` execution
evidence. Missing options, expired reviews and wrong signatures fail. This composes
review admission and pair verification; it does not update an accepted Git ref or
complete an issue. Retain original proof bytes for future revalidation.


Retain the original pair alongside the green report:

```sh
workdeck ci import-report --report-file green-report.json --policy-file producers.json \
  --expected-policy "$PRODUCER_POLICY_HASH" --expected-commit "$GREEN_COMMIT" \
  --red-green-file retained-pair.json --actor "$ACTOR" --request-id "$REQUEST_ID" --json
workdeck ci reauthenticate-red-green "$ATTESTATION_ID" --authority-file current-authority.json --json
```

`retained-pair.json` uses the `retained-red-green-proof` schema: `baseline`, `check`,
`red` signed envelope, `red_artifact` and `green_artifact` exact XML strings, and an
optional `review` containing the original baseline review. Obtain its baseline and
policy pins independently before import. `current-authority.json` uses the
`retained-red-green-authority` schema: independently accepted `baseline`, exact
`candidate`, current producer `policy`/`expected_policy`, and current `review`
authority when the stored proof was reviewed. Use protocol schemas for the complete
typed structures. Do not copy historical import policy into current authority without
an independent trust decision.

The entire retained record must fit 8 MiB. The normal `ci report` operation returns
its original proof; `ci reports` describes historical green-report admission and does
not grant pair qualification. Offline snapshot restore preserves proof without Git.
Fresh pair reauthentication requires its immutable Git objects and original signed
proof. Retention, replay and historical reads do not renew trust or complete work.


To connect a criterion declaration to retained execution proof, include one exact
link in its `DeclareEvidence` input:

```json
{"kind":"attestation","id":"ATST-<ULID>","content":"sha256:<record-hash>"}
```

Resolve a project criterion using `workdeck gate criterion project PROJECT_ID CRITERION_ID`.
The `issue`, `feature` and `milestone` owner variants use the same command shape.
Project criteria are `exit_criteria`; milestone criteria are `outcomes`. For red/green
source evidence, use the verified pair's `candidate.content` as `subject.content`,
its check and green producer references, the green run ID as `result.id`, and the
original signed report's fingerprint and `observed_at`. The original observation
time must not be refreshed when declaring or re-verifying evidence.

After `evidence declare`, verify the exact resulting declaration:

```sh
workdeck evidence verify-red-green "$EVIDENCE_ID" \
  --expected-evidence-content "$EVIDENCE_HASH" --authority-file current-authority.json --json
```

This read-only command fails on missing/expired/superseded declarations, wrong source
or proof pins, mismatched producer/check/result/time, changed criteria, or a source
edit during verification. It returns `authenticated_check_link` with current and
committed criterion records and the freshly verified original proof. This is not a
Done, gate-acceptance or project-exit decision; completion-policy integration remains
required. Use `protocol render schemas` for complete input and output schemas.


To qualify an entire committed AND gate, build `red-green-gate-request` JSON using
independently obtained authority, the gate's exact inspected source token, and one
pinned evidence selection for each requirement, then run:

```sh
workdeck gate verify-red-green gate-proof.json --json
```

Use `workdeck schema red-green-gate-request --json` to inspect the input contract.
Every required proof must match the exact committed gate, criterion, check and
producer. Missing requirements, changed gates, conflicting pins, stale evidence and
expired authority fail. The `authenticated_requirements` output is read-only and does
not complete issues or serve as a reusable completion credential.

For a gate backed by passed imported checks without a red/green pair, build a
`verified-gate-request` with the exact gate/evidence selections and independent
`completion-authority`, then run:

```sh
workdeck schema verified-gate-request --json
workdeck gate verify-green gate-proof.json --json
```

This applies the same current/committed gate, criterion, producer, source, freshness
and attestation checks. Configured or retained red/green records still require their
original pair authority. The result is read-only; verified issue completion can retain
the assessment in its historical receipt.

For required red/green checks, completion uses an explicit proof request rather than
an ordinary/manual Done declaration:

```sh
workdeck schema complete-red-green-issue --json
workdeck issue done ISSUE_ID --verification-file completion.json --dry-run --json
workdeck issue --request-id REQUEST_ID done ISSUE_ID --verification-file completion.json --json
```

The file supplies the issue ID/source token, actor, independent baseline/candidate
and producer/reviewer authority, each required check's original attestation ID/content
pin, and selections for every attached gate. The candidate must be current HEAD;
planning definitions, issue requirements, execution inputs, parameters, environment
and tools must match the checked source. Missing checks/profiles/gates and stale
inputs fail before publication. `--dry-run` does not write; manual-acceptance flags
cannot be combined with this proof file.

Exact request retries return the original completion receipt, including after Git
becomes unavailable. The retained receipt is historical evidence and does not renew
producer/reviewer authority or qualify a later source. This path currently requires
original red/green proof for every required check. For an existing claim, the
authenticated green-only variant keeps the ownership and proof together:

```sh
workdeck schema claimed-verified-completion-input --json
workdeck issue complete ISSUE_ID --verification-file verification.json --json
```

The verification file must name the current claim's actor, issue and source and must
contain the same current-source, check and gate proof accepted by verified issue
completion. The operation rechecks claim ownership and captured inputs under the
transaction lock, retains the proof for historical replay and rejects release flags
on this path. The TUI exposes the same policy through `p` in Projects/Milestones
and `m` in Features; independent authenticated policy bases remain in progress.

Project and milestone exits and feature maturity use the same source-bound policy
evaluator. Assessments are read-only and expose every blocking condition, source
pin, deterministic fingerprint, and policy basis. Mutations require an expected
source token and an attributed acceptance; the transaction re-evaluates the live
snapshot before writing the normal receipt.

```sh
workdeck project assess PROJECT_ID --json
workdeck project complete PROJECT_ID --actor ACTOR --reason REASON \
  --expected-revision REVISION --expected-content CONTENT --json
workdeck milestone assess MILESTONE_ID --json
workdeck milestone complete MILESTONE_ID --actor ACTOR --reason REASON \
  --expected-revision REVISION --expected-content CONTENT --json
workdeck feature assess FEATURE_ID --to specified --json
workdeck feature promote FEATURE_ID --to implemented --actor ACTOR --reason REASON \
  --expected-revision REVISION --expected-content CONTENT --json
```

Project/milestone completion requires completed issue members and explicit criteria;
feature promotion advances one stage at a time and requires accepted criteria,
completed associated issues, and mature prerequisites at `implemented`. Declared
feature gates remain unknown until the authenticated gate path is supplied.
Manual acceptance, local feedback, reviewed evidence, and CI qualification remain
separate bases; a policy assessment is not a reusable completion credential.

Maintainer gates use the checked-in named release profile and validator pin:

```sh
cargo xtask pm profile --profile standalone
cargo xtask pm check --profile standalone
cargo xtask pm performance --profile standalone
cargo xtask pm release-check --profile standalone
```

The profile rejects validator-source drift and unsafe/non-Cargo commands before
running the PM suite, CLI compatibility targets, calibrated full-size projection
benchmarks, and the synthetic dogfood journey. Its trusted-baseline identifier records the independent review
anchor; local profile execution remains development evidence until that anchor is
verified by the repository's external review process.

Qualify a pinned imported check against the current source and inputs:

```sh
workdeck schema verify-imported-check --json
workdeck ci verify-imported-check verification.json --json
```

The request includes independent producer authority and exact attestation pins.
A signed failed check is rejected. Checks configured for red/green also require
original pair proof and current authority. This read-only command does not transition
an issue. For a mutation, use the same attestation selections in a
`complete-verified-issue` request with `issue done --verification-file`; the transaction
rechecks the exact current plan, source, invocation inputs and required-check denominator.
The verified mutation can retain either the legacy red/green gate assessment or the
authenticated green-only assessment from `gate verify-green`. Claimed verified
completion retains the same proof while fencing the current ownership claim.
