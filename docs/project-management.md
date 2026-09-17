# Workdeck Git-native project management

Status: Supporting design and implementation overview; current implementation progress and qualification are tracked in the standalone implementation plan.
Recorded: 2026-09-08.
Scope: Workdeck's repository project-management system, with broader integrations retained as future design context.

The current [standalone implementation plan](project-management-implementation-plan.md) excludes
Bowerbird work and owns the active phase sequence and acceptance criteria. Its scope supersedes
the earlier integration-oriented sequencing in this document. External-product sections below
remain future design context rather than current implementation requirements.

## Purpose and decision status

Expand Workdeck's prototype issue tracker into a Git-native engineering project manager that
connects planning, issues, source review, and implementation evidence. All Workdeck-owned
repository planning data belongs under **`.workdeck/` at the repository root**.

The root location is an explicit user requirement. The object model, concurrency protocol,
architecture, and implementation sequence below are recommendations recorded for review. This
document does not claim that the proposed behavior exists, authorize implementation or commits,
or change another product's authority by itself.

The original supplied [Git-native PM specification v1](reference/git-native-pm-spec-v1.md) is
preserved verbatim as design input. Its `.project/` directory and `pm` binary names do not apply
to Workdeck. Where this proposal differs from that reference, the differences are explicit below.
Instructions to implement or commit inside the reference are historical source text, not the
current task scope.

This is the consolidated design for the revamp. Earlier [Workdeck planning](workdeck-plan.md)
and [files/CLI planning](files-ux-and-cli-plan.md) remain useful prototype history; their issue
storage and workflow proposals should not be used as competing definitions for this redesign.

## 1. Product boundaries

Workdeck remains one Rust terminal product with one shipped executable, `workdeck`. Its CLI and
TUI expose the same project-management operations. No required server, browser UI, separate `pm`
binary, or coding-agent process host is introduced.

Files are authoritative. Git records accepted history and transports collaboration. SQLite,
search indexes, UI selections, unread state, presence, and caches are disposable projections.
Deleting the index and rebuilding it must reproduce the same view of the same source snapshot.

Three related layers must remain distinct:

| Layer | Objects | Meaning |
| --- | --- | --- |
| Product definition | Features, components, typed dependencies, quality gates | What the product should support and what actually exists |
| Planned work | Initiatives, projects, milestones, issues, cycles | What people and agents intend to deliver and who owns the work |
| Delivery evidence | Commits, reviews, checks, runs, artifacts, releases | What changed and what supports the result |

Completing an issue does not automatically ship a feature. A feature can be implemented while
qualification remains outstanding. A successful agent run can produce a candidate requiring
review. Keep these facts visible rather than compressing them into one progress percentage.

| System | Recommended responsibility |
| --- | --- |
| Workdeck | Repository engineering plans, issues, acceptance criteria, Git workflow, and terminal review |
| Bowerbird Feature Tree | Bowerbird capability identities, dependencies, maturity, gates, and evidence declarations |
| Bowerbird runtime | Durable execution, verification, artifacts, receipts, and recovery |
| OpenCompany | Organizational outcomes, business work, business approvals, and collaboration |
| Herdr | Interactive agent sessions, terminals, processes, and worktree execution |
| Git | Repository history, source revisions, and accepted file changes |

Existing [Workdeck product boundaries](PRODUCT_BOUNDARIES.md) continue to govern implementation.
Bowerbird's current documents assign organizational WorkItems to OpenCompany. Giving Workdeck
repository engineering issues is a proposed refinement: business WorkItems link to engineering
issues, rather than becoming two writable copies of the same ticket. Bowerbird's documents must
be updated explicitly when that refinement is adopted. Its runtime and other products' private
databases must not become dependencies of Workdeck.

## 2. Current implementation baseline

The inspected Workdeck branch was `linear-project-management`, at commit `c9a36ec6` before this
documentation change. The relevant existing seams are:

| Location | Current behavior | Implication for the revamp |
| --- | --- | --- |
| `crates/workdeck-cli/src/store/mod.rs` | TOML issue model, projects/cycles/labels, store writes, append-only activity | Extract PM behavior from the CLI; add a deliberate migration |
| `crates/workdeck-cli/src/main.rs` | Issue command parsing and orchestration; startup chooses between two TUI paths | Keep command compatibility and remove the startup-dependent planning experience |
| `crates/workdeck-cli/src/app.rs` | Selection, issue mutations, file linking, previews | Replace direct persistence calls with shared application operations |
| `crates/workdeck-cli/src/tui/mod.rs` and `views/mod.rs` | Tabbed issue list and key handling | Port useful behavior into the unified workbench |
| `crates/workdeck-cli/src/search/mod.rs` | In-memory search over repository and issue records | Share the future PM query model and projection |
| `crates/workdeck-cli/src/config.rs` | TOML application configuration and `.agents/workdeck` default | Separate PM schema settings from existing app settings; migrate paths explicitly |
| `crates/workdeck-tui` | Native continuous diff-review interface | Host persistent planning navigation without regressing review behavior |
| `crates/workdeck-store` | Independent durable local app-state persistence | Keep disposable/local app state distinct from authoritative PM records |
| `crates/workdeck-migration` | Existing Hunk configuration migration | Extend migration deliberately; do not redirect older imports accidentally |
| `xtask/src/architecture.rs` | Executable crate dependency and source ownership rules | Update intended boundaries alongside new crate wiring |

Today each issue is `.agents/workdeck/issues/WD-N.toml`. Status and priority are fixed enums;
projects, cycles, assignee, due date, labels, files, and commits are supported. CLI issue CRUD,
filters, JSON, and file/commit links provide useful compatibility behavior. There is no native
Linear or GitHub issue synchronization.

Default startup currently opens the continuous review UI when a changeset exists and the older
tabbed UI otherwise. Issues therefore are not consistently accessible from the primary review
surface. Merely extending the old Issues tab would preserve this product defect.

## 3. Planning and capability model

| Object | Contract |
| --- | --- |
| Repository | Stable identity independent of clone and local filesystem location |
| Initiative | Larger outcome spanning projects |
| Project | Bounded deliverable with a lead, scope, dependencies, and exit criteria |
| Milestone | Verifiable checkpoint inside a project |
| Issue | Independently assignable and reviewable work package |
| Parent issue / epic | Work breakdown containing child issues |
| Cycle | Timebox for scheduling work, independent of release acceptance |
| Target / release | Delivery destination or overlapping required-capability set |
| Feature | Lasting capability, component, or other typed inventory node |
| Quality gate | Requirement for accepting a capability or delivery milestone |
| Evidence reference | Result bound to an exact source, artifact, or execution subject |

Projects, milestones, cycles, issues, and features have distinct identities. IDs survive renames.
Repository-qualified references prevent identically named issues in different repositories from
colliding. A local repository catalog maps stable identities to machine-specific paths.

One issue can advance several features; one feature can require multiple implementation,
documentation, integration, and qualification issues. Do not generate one ticket per capability
node. Create issues only when work is specified enough to execute and independently verify.

Simple repositories can use only issues and labels. The larger object model must be optional in
the UX, not a mandatory configuration exercise.

### Issue fields

Use YAML frontmatter plus a Markdown body. The structured model should include:

- Immutable ID, schema version, revision, title, type, and workflow status.
- Priority, project, milestone, cycle, parent, labels, estimates, and due date.
- Assignee, reporter, reviewer, creation/update/closure timestamps.
- Feature references and explicit issue prerequisites.
- File, commit, pull-request, design-document, and evidence links.
- Validated custom fields under `custom:` and namespaced optional external references.

The Markdown body contains purpose, requirements, reproduction steps where relevant, acceptance
criteria, and notes. Templates provide useful structure without manufacturing requirements.
Estimate units are explicit; points and hours must not be summed as interchangeable values.

Each comment, time entry, and evidence reference has its own unique identity. Corrections preserve
history through Git and explicit amendment/supersession where the record requires it. Authors
are attribution; a Git name or email is not an authorization mechanism.

### Identifiers

New issues use a configured prefix plus a full ULID. Directory names contain the immutable ID,
never the title or status. The CLI accepts an unambiguous abbreviation and rejects ambiguity;
projections display a short ID alongside the title. The complete ID remains canonical.

Existing `WD-N` issue identities survive migration. Deletions use tombstones/archive semantics
when references or history need to remain resolvable; identities are never reused. Sequential
allocation is deferred until there is a justified need and a serialized allocation protocol.

## 4. Repository layout and configuration

```text
.workdeck/
├── config.yml                   # Versioned PM configuration
├── config.toml                  # Existing app configuration format, if repo overrides exist
├── schema.yml                   # Custom field declarations and validation
├── users.yml
├── labels.yml
├── AGENT-PROTOCOL.md
├── templates/
├── initiatives/
├── projects/
├── milestones/
├── cycles/
├── views/
├── issues/
│   └── BWB-<ULID>/
│       ├── item.md
│       ├── comments/<ULID>.md
│       ├── time/<ULID>.yml
│       ├── evidence/<ULID>.yml
│       └── attachments/
├── features/                    # Native feature provider, when enabled
├── gates/
├── wiki/
├── .index/                      # Ignored, disposable projection
├── .tmp/                        # Ignored, temporary writes/recovery data
└── settings.local.yml           # Ignored, per-machine PM preferences
```

The TOML app configuration and YAML PM configuration have separate parsers and ownership. This
avoids silently changing the existing theme, review, extension, and keybinding configuration
format. User-wide application settings retain their existing location. The implementation must
document deterministic discovery and precedence rather than maintain two PM configurations.

Existing documentation can stay in `docs/`; Workdeck links to it. A repository need not relocate
its architecture library into `.workdeck/wiki/`. Large or sensitive evidence stays in its owning
artifact system and is referenced, rather than copied into Git.

Claims use `.workdeck/claims/<issue-id>.yml` in the configured coordination worktree when shared
coordination is enabled. That directory is not a second writable copy of issue definitions.
Local single-writer mode must be explicitly distinguished from shared claims.

Initialization writes the required ignore entries for `.index/`, `.tmp/`, and local settings.
It does not overwrite existing configuration, install hooks silently, auto-stage unrelated files,
or create every optional object directory unnecessarily. Read commands never initialize state.

## 5. Changes to the supplied specification

| Reference design | Workdeck recommendation | Reason |
| --- | --- | --- |
| `.project/`, `pm` binary | `.workdeck/`, existing `workdeck` binary | Explicit product direction and existing architecture |
| Store both `blocks` and `blocked_by` | Store one prerequisite direction and derive the inverse | Avoid two-file relation updates and contradictory edges |
| Monthly shared JSONL time log | One file per time entry with an immutable ID | Avoid shared append contention and heuristic deduplication |
| Comment ID from timestamp/author | Unique ID; time and author are metadata | Same actor can comment concurrently |
| Update `item.md` for every comment | Derive `last_activity_at`; only item edits change its revision | Separate comment files should actually avoid item conflicts |
| File last-write-wins | Expected-revision/content checks and explicit conflicts | Prevent lost updates |
| Remove dependencies when canceling | Preserve relations and explicitly resolve or waive prerequisites | Canceled work does not prove a required result |
| Claim fields inside issue frontmatter | Separate claim records | Renewal should not rewrite acceptance criteria |
| Automatic staging enabled | Disabled by default; explicit scoped staging | Preserve normal review and mixed-index workflows |
| Expired claims return directly to ready work | Expose recovery/reconciliation state | Previous execution may still be active or have produced results |
| Claim all backlog items | Select explicitly ready, eligible, unblocked work | Scheduling should not implicitly authorize unspecified work |
| CLI-only generic v1 | Shared CLI core followed by integrated Workdeck TUI | Workdeck already has a terminal product surface |

Git performs three-way merges and can leave conflicts; it does not automatically implement
last-write-wins. Later semantic merge support should compare base/ours/theirs and retain a
conflict when both sides assign different values to the same field. It must not blindly union
dependency sets or claim records. See the [Git merge documentation](https://git-scm.com/docs/git-merge).

Parent and hard-dependency cycles are errors. Relation direction is explicit. Canceled blockers
require a decision, while completed blockers satisfy an issue prerequisite according to policy.
Missing references and invalid records remain visible diagnostics. `doctor --fix` should show a
bounded repair plan and must not delete relationships to make validation pass.

## 6. Workflow, acceptance, and evidence

Recommended engineering workflow:

```text
Inbox → Backlog → Ready → In progress → In review → Verification → Done
                                                                Canceled
```

Transitions are configured and validated. Custom statuses map to stable semantic categories so
queries and automation do not depend on English labels. Repositories can use fewer stages.
Blocked is primarily a derived condition plus explicit blocker reasons; an issue can remain in
review while blocked on another decision.

Feature maturity is separate from issue workflow. Bowerbird already distinguishes decision,
maturity, work state, and availability. Preserve its transitions rather than mapping all of them
onto the issue board. `implemented`, `qualified`, and `shipped` require different evidence.

Completion evaluates the applicable policy:

- Required acceptance criteria are satisfied without deleting or weakening requirements.
- Required children and prerequisites are resolved; cancellation is not automatic satisfaction.
- Required review and verification results exist for the relevant subject.
- Evidence has adequate provenance and is current where freshness is required.
- Closure records the result and clears the appropriate active claim through the claim protocol.

Acceptance parsing must distinguish real task items from code examples and unrelated checklists.
Use stable criterion references when evidence targets individual criteria. Empty placeholder
checkboxes are not a meaningful completion contract.

Evidence records reference exact commit/artifact identities, check definitions, outcomes,
provenance, and relevant freshness rules. Merely attaching a link or checking a box is not
verification. Workdeck stores references and evaluates declared policy; Bowerbird produces its
execution evidence and owns verification runtime behavior.

Changes to accepted requirements are reviewable changes. An agent proposing implementation must
not silently weaken the requirements against which that implementation is judged.

## 7. Mutation and concurrency contract

All Workdeck writers use one application API. Humans can still edit files directly; validation
reports malformed edits, and CI enforces accepted-state invariants. Do not claim a local library
can prevent every write by a person who can modify the repository.

Every application mutation should:

1. Resolve the repository, configured planning source, and current source snapshot.
2. Load and validate relevant records and expected revision/content identities.
3. Validate workflow, references, and operation preconditions.
4. Build an explicit change set limited to touched files.
5. Serialize local writers and recheck preconditions before replacing files.
6. Write using unique temporary paths and atomic replacement per file.
7. Recover or reject incomplete multi-file operations explicitly.
8. Invalidate the relevant projection and return a structured receipt.

A temporary-file rename is not a cross-process compare-and-set or a multi-file transaction.
Expected revisions require an actual serialized check/write boundary. Direct editor changes must
also be detected through source content, even if the editor did not increment `revision`.

Use minimal field edits where feasible, stable field order, preserved Markdown content, strict
duplicate-key handling, and bounded parsing. Unsupported schema versions are explicit errors.
Keep custom metadata rather than silently discarding it during migration or serialization.

Automation operations accept an idempotency identity. Replaying the same operation must not add
duplicate comments, evidence, or time entries. Receipts identify operation, touched files, new
record revision, source snapshot, and local/published/uncertain publication state. Persist the
minimum durable operation identity needed to reconcile a retry; a disposable SQLite row alone
cannot provide durable deduplication.

## 8. Git sources, branches, and worktrees

The recommended Bowerbird profile separates accepted content from task coordination:

| Content | Authority |
| --- | --- |
| Accepted issues, requirements, feature declarations, evidence declarations | Reviewed canonical repository branch |
| Proposed requirements and implementation changes | Implementation branch, reviewable together |
| Short-lived claims | Dedicated coordination ref and worktree |
| Local drafts, UI state, index | Explicit local files or disposable machine state |

The UI identifies repository, source, branch/ref, revision, and freshness. It distinguishes
accepted state, branch proposals, and confirmed claims. A claim can produce an active-work
indicator without rewriting accepted feature maturity or claiming that a pending status proposal
has already been merged.

Support an explicitly configured dedicated PM branch or sibling planning repository as another
mode. Each record has one authority; Workdeck must not combine two writable backlogs silently.
Non-Git VCS review remains available, while shared PM publication initially uses a Git transport.
Keep the source interface extensible without claiming Jujutsu/Sapling claim parity in v1.

Git publication operates in the configured PM/coordination context. It must not automatically
rebase a developer's dirty code worktree, commit unrelated files, bypass protected branches, or
force-push accepted history. Optional staging is scoped; commit and publication are explicit
operations or part of an explicitly invoked publishing command.

Client validation and optional hooks improve feedback. They are not a substitute for remote
validation when accepting changes from multiple writers.

## 9. Agent selection and claim protocol

`next` selects a ready, specified, unblocked, actor-eligible work package. Scope can include
project, milestone, cycle, target, labels, capabilities, and required feature/gate state. Return
why an issue is eligible and why excluded candidates are blocked. Selection never invents work
or creates tickets from the feature inventory without a separate planning operation.

Shared claim acquisition:

1. Fetch the configured canonical and coordination state.
2. Read the accepted issue revision and existing claim.
3. Refuse a current foreign claim; distinguish recovery from fresh pickup.
4. Create a unique token binding actor, issue revision, and expiration.
5. Publish against the observed coordination-ref version.
6. On rejection, reload current state and reevaluate ownership before retrying.
7. Confirm acquisition before authorizing execution under the shared protocol.

Remote ref update checks can underpin publication; application logic still enforces issue
eligibility and ownership. See [Git push documentation](https://git-scm.com/docs/git-push).
The final implementation must specify the exact publication operation and test it against two
independent clones. Blindly rebasing and replaying an old claim is not sufficient.

Renew, release, and agent completion require the current claim token. An uncertain network result
is reconciled by inspecting the shared record before reporting acquisition or retrying. Offline
claims are labeled local/uncoordinated and cannot promise cross-machine exclusivity.

An expired claim exposes abandoned work for recovery. It does not prove that the old process
stopped. Clock/lease policy and revision drift must be explicit. Bowerbird owns execution fencing
for its Runs; Herdr owns interactive process lifecycle. Workdeck owns task coordination only.

Agents write back a summary, actual test commands/results, source references, and applicable
acceptance evidence. A direct file edit can propose work offline, but it cannot substitute for
the shared claim publication protocol. Keep `AGENTS.md` thin and link the generated protocol;
initialization must merge a pointer without overwriting unrelated repository instructions.

## 10. Bowerbird integration and scale

The current local Bowerbird checkout is named `openfactory`; its product name is Bowerbird.
The inspected committed head was `c4e9d01`, with a substantial uncommitted documentation
reorganization. Findings describe the inspected working tree, not a clean release snapshot.

On 2026-09-08, `python3 tools/feature_tree.py summary` and `validate` reported:

- 38,276 total nodes, including 22 quality gates.
- 6 nodes at implemented maturity, 1,582 fixture-backed, 36,109 specified, and 579 retired.
- A valid feature tree under the current bootstrap validator.

Validation is structural evidence, not proof that the planned platform exists. The repository has
a greeting-only Rust CLI and a small set of implemented foundational value types; its factory
runtime is not implemented. Do not convert its inventory size into a feature-completion claim.

Relevant Bowerbird sources, relative to that repository:

- `docs/planning/FEATURE_TREE.md`: canonical capability, dependency, gate, status, and evidence nodes.
- `docs/planning/MASTER_PLAN.md`: initiative/project/milestone hierarchy and crosswalk.
- `tools/feature_tree.py`: validation, deterministic projections, and revision-checked mutations.
- `docs/planning/strategy/portfolio-organization-and-integration.md`: current product authorities.
- `docs/planning/designs/operator-surfaces/cli-and-mcp.md`: proposed pipeline, release, operation, and evidence contracts.
- `docs/planning/designs/runtime/context-and-evidence.md`: proposed evidence ledger and context-pack compiler.
- `AGENTS.md`: repository evidence and reviewed-change requirements.

### Adapter first

Implement a read-only feature source adapter preserving existing `BWB-F…` and `BWB-Q…` IDs,
node kinds, hierarchy, typed dependencies, gate/evidence fields, target sets, and project/milestone
links. New Workdeck issues link to these IDs. Do not create a second writable copy in
`.workdeck/features/`.

Later feature mutations produce reviewed, expected-revision proposals using Bowerbird's existing
rules. Adapter commands, if used, are explicitly configured integrations, not arbitrary commands
automatically trusted because a repository file mentions them.

A future migration may make per-node `.workdeck/features/<ID>.md` files authoritative and generate
the large Markdown tree. That requires a coordinated update to Bowerbird's tools, documentation,
and authority contract, preserving IDs, tombstones, semantics, and evidence. It is a separate
migration, not a prerequisite for useful Workdeck issues.

### Derived views and performance

Indexing is an early requirement for this consumer. Use incremental file/source invalidation,
source fingerprints, indexed graph edges, and virtualized/collapsed views. Benchmark against at
least a synthetic 40,000-node capability graph and a separate active issue workload.

The index records which source snapshot it represents. Invalid files and stale last-good data
are visible; they must not be presented as current authoritative state. Rebuild deterministically
from files. Cross-repository aggregation preserves source-qualified identities and freshness.

Only hard dependencies determine build order. Preserve other edge kinds without turning them
into blockers. Bowerbird's current `critical_path` computes a longest hard-dependency chain, not a
duration-based schedule. Label it as a dependency path unless duration data and scheduling rules
justify a delivery forecast. Preserve prerequisites outside a selected target when explaining
readiness, even if the visual view is filtered to that target.

## 11. Terminal product experience

Use one persistent workbench shell. Planning remains reachable with or without code changes.
Keep explicit pager/patch/stdin review entrypoints focused on their existing purpose.

| View | Primary use |
| --- | --- |
| My work | Assigned, claimed, review-requested, blocked, and overdue work |
| Issues | Filterable/grouped list, detail view, bulk operations, optional board |
| Projects | Scope, milestones, active work, dependencies, and exit criteria |
| Features | Collapsible capability graph/tree with maturity and evidence |
| Cycles | Commitment, carryover, and capacity when estimates are meaningful |
| Review | Code changes alongside the linked issue and acceptance criteria |
| Activity | Comments, relevant Git changes, ownership, and evidence updates |

The first complete interaction is: open an issue, inspect acceptance, jump to files/PR, inspect
the diff and verification evidence, and return without losing issue or review context.
Creating an issue from a file or review note should capture useful links and source identity.

Saved views are versioned queries, not stored copies of boards. Support grouping, sorting,
project/target/cycle/label filters, and useful empty/error states. Keep selection, scroll position,
expanded nodes, and panel sizes in local app state. Narrow panes need useful list/detail modes;
wider terminals can show planning and review together. Keybindings remain contextual and
discoverable, including existing review-navigation compatibility.

Feature inventories must not become the entire navigation hierarchy. Show target/project slices,
readiness, missing evidence, and blockers. Report issue throughput separately from capability
maturity and release qualification. Do not advertise a raw node-count percentage as delivery
progress.

## 12. Workdeck implementation architecture

### Recommended package boundary

Introduce one cohesive `workdeck-pm` library with no CLI, terminal, or Bowerbird-runtime dependency.
Start with modules rather than a large collection of new crates:

```text
workdeck-pm/
  model          IDs, records, workflow categories, references
  schema         versioning, custom fields, validation
  document       frontmatter/Markdown parse and minimal edit
  repository     source discovery, record reads, change sets
  mutation       preconditions, locking, receipts, recovery
  workflow       transitions, acceptance, completion policy
  graph          parents, prerequisites, gates, derived blockers
  query          filters, saved views, readiness explanations
  projection     rebuildable index and source fingerprints
  coordination   claim state machine and publication interface
  sources        native records and optional feature providers
```

These are proposed responsibilities, not frozen module filenames or public API names. Keep pure
domain logic separate from filesystem/index adapters inside the package. Extract another crate
only when a concrete reuse or dependency boundary warrants it.

`workdeck-cli` remains the composition root, connects Git/publication adapters, parses commands,
and renders structured/text results. `workdeck-tui` consumes PM view models and application
actions; it must not perform direct YAML or SQL writes. Avoid embedding another command parser
inside the TUI.

`workdeck-store` retains machine/local app-state behavior. Its current forgiving treatment of
missing/malformed app state is inappropriate for authoritative issue records; do not reuse it
unchanged as a PM database. `workdeck-review` retains review semantics and exchanges explicit
references with PM. `workdeck-session` retains its existing review-session responsibilities and
does not become an issue-claim or coding-agent runtime.

Adding `workdeck-pm`, CLI/TUI edges, or a shared interface requires coordinated changes to the
Cargo workspace, architecture checker, and architecture documentation. Preserve the zero-known-
violation policy instead of adding baseline exceptions.

### CLI direction

Keep the existing command families and established aliases where practical. Proposed additions:

```text
workdeck init
workdeck doctor [--staged]
workdeck reindex

workdeck issue create|list|show|update|edit
workdeck issue comment|relate|unrelate
workdeck issue next|claim|renew|release|done|reopen|cancel
workdeck issue link-file|unlink-file|link-commit|unlink-commit

workdeck project list|show|create|update
workdeck milestone list|show|create|update
workdeck cycle list|show|create|update
workdeck feature list|show|deps|gates|evidence
workdeck view list|show
workdeck board
workdeck time log|report
```

These are proposed commands. Existing `--init`, issue `close`, JSON envelopes, and other supported
interfaces need a compatibility map before being changed. `close` should converge on the same
completion policy as `done`, rather than bypassing it. CLI convenience commands delegate to
semantic operations, not unrestricted field assignment.

Version JSON contracts, return structured errors and publication state, support body/JSON input
through files or stdin, and make dry-run/expected-revision semantics consistent. Preserve current
commands through adapters during extraction; remove duplicate mutation implementations once
callers are migrated.

## 13. Implementation increments

The [standalone phased implementation plan](project-management-implementation-plan.md) is the
current execution plan. It replaces the initial W0–W7 proposal with PM-00–PM-12, covering the
native PM core, migration, persistent issue/review workbench, planning and feature models, agent
context, local commands/checks, Git claims, indexing, CI validation, and release qualification.

All phases are currently planned. No Bowerbird adapter, runtime bridge, migration, or pilot is
included. The phase document owns dependencies, review slices, and testable exit criteria; do not
maintain a competing completion table here.

The first vertical implementation establishes the library/schema seam, explicit `.workdeck/`
initialization, Markdown issue create/list/show/update, and invalid/stale-write protection. The
persistent planning/review UI and legacy cutover arrive in the first preview milestone. See the
[phase details](project-management-implementation-plan.md).

## 14. Migration and compatibility

Migration is explicit, previewable, and repeatable. Inventory the legacy store and configuration,
show source-to-destination mappings, convert into a staged destination, validate, then switch
discovery according to the migration contract. Preserve source data until successful verification
and an explicit cleanup step; never require destructive cleanup to inspect the result.

Preserve IDs, titles, descriptions, timestamps, priorities, assignees, project/cycle/label links,
file/commit links, and custom TOML metadata. Map old status names to the selected workflow with
an explicit table. Unknown values are diagnostics, not silent defaults. References to legacy IDs
continue to resolve.

Both stores present without a completed migration produce an ambiguity diagnostic. Do not merge
them silently or continue writing each from a different UI path. Validate reruns and partial
migration recovery. Existing app configuration remains TOML; moving repository overrides must
preserve their values and avoid rewriting global preferences.

The root migration must inventory all `.agents/workdeck` consumers, including existing Hunk
migration targets, handoffs, imported session metadata, review references, examples, tests, and
generated documentation. Each gets an explicit mapping or documented compatibility treatment.
Imported sessions and app state are not issue records and must not be converted with the issue
serializer. A simple recursive directory move is insufficient.

## 15. Validation and release criteria

Use temporary repositories for mutation and Git tests. Never exercise migration or competing
claims against a user's live repository as a test fixture.

Required behavioral coverage includes:

- Markdown/YAML round trips, minimal diffs, duplicate keys, custom fields, Unicode, and unsupported schemas.
- Read commands leave an uninitialized repository untouched.
- Simultaneous local writes, source edits outside the CLI, and stale revision/content rejection.
- Parent/hard-dependency cycles, dangling references, canceled prerequisites, and incomplete children.
- Independent comments/time/evidence records merge without a shared item timestamp hotspot.
- Idempotent retry, interrupted writes, recoverable multi-file changes, and scoped staging.
- Completion rejected for missing or wrong-subject evidence and unfulfilled acceptance policy.
- Two clones racing claims, rejected/ambiguous publication, renewal, stale release, and expired-owner recovery.
- Canonical/branch-overlay distinctions and planning visibility across code worktrees.
- Index deletion/rebuild parity, stale-source diagnostics, incremental refresh, and a 40,000-node graph.
- Legacy migration reruns, collisions, metadata preservation, and partial failure recovery.
- TUI issue/review navigation with clean and dirty repositories, narrow/wide layouts, and no review regressions.

Run focused crate/CLI/PTY checks per increment, architecture checks whenever dependencies change,
and existing workspace/release gates for the integrated change. Do not claim Bowerbird runtime
verification from source-contract or adapter tests.

The initial Bowerbird adoption proof is one bounded implementation package linked to existing
feature IDs: acquire its claim from another worktree, implement it, review the diff, attach
exact-source evidence, close the issue, and propose the justified feature-maturity change.
An issue becoming Done must not automatically claim the feature is shipped.

## 16. Deferred work and remaining design details

Defer hosted/web UI, presence, CRDT editing, two-way SaaS synchronization, sequential ID allocation,
automatic event-sourced issue snapshots, and Bowerbird's feature-storage migration. Workdeck's
current terminal-only boundary continues to exclude a hosted web product even though the generic
reference spec describes one as a possible evolution.

Before the relevant implementation increment, settle:

- Exact public schema, workflow-category vocabulary, revision/content token, and JSON compatibility.
- Per-entity authority and accepted-versus-branch overlay resolution.
- Coordination-ref layout, lease time policy, publication primitive, and recovery state machine.
- Durable operation receipt representation and crash recovery for multiple files.
- Completion/evidence profiles, criterion identity, and policy changes against in-flight work.
- Source adapter activation, caching, and proposal contracts.
- Navigation/keybinding details that coexist with the continuous review surface.

These details are bounded implementation design work. They do not require reopening the chosen
root directory, file authority, separate feature/issue identities, or the single Workdeck binary.

## 17. Agent experience and the engineering operating-system direction

Design addition recorded on 2026-09-08 after the initial implementation overview. These are
proposed product contracts and commands, not implemented capabilities or a claim that the
combined platform is operational.

Workdeck plus Bowerbird can form a coherent agentic engineering operating system: repository
intent, bounded work, context, verification, review, delivery, and feedback are connected by
stable identities and evidence. The value comes from making the whole loop dependable and
understandable to an unfamiliar agent.

```text
Product intent and features
          ↓
Ready issue + accepted acceptance contract
          ↓
Claim + bounded repository context + execution binding
          ↓
Implementation → fast feedback → review → exact-source qualification
          ↓
Release plan → delivery → observed outcome
          ↓
Evidence, feature-maturity proposal, and bounded follow-up work
```

Each arrow has explicit input/output identities and can be inspected or resumed. Durable work
belongs to Bowerbird; interactive coding processes belong to the selected agent runtime. Workdeck
provides the repository interface and task/evidence relationships. The product must remain useful
for local planning and verification while Bowerbird's runtime is unavailable.

### Agent-facing capabilities to prioritize

| Capability | Proposed surface | Agent benefit |
| --- | --- | --- |
| Discover the environment | `workdeck capabilities --json` | Know supported commands, schema versions, providers, current repository, and unavailable capabilities |
| Obtain bounded task context | `workdeck context --issue <ID> --budget <N> --json` | Start with relevant requirements, code/docs links, commands, blockers, and evidence |
| Explain the next action | `workdeck next --issue <ID> --json` | Distinguish implement, test, review, resolve a question, reconcile, or no eligible action |
| Discover executable recipes | `workdeck command list/show` | Avoid repeatedly guessing setup/build/test/lint commands |
| Plan required verification | `workdeck check plan --issue <ID>` | Know which checks apply, why, and what subject they evaluate |
| Inspect structured failures | `workdeck check results --failed --json` | Receive actionable failures, source locations, and report references |
| Resume interrupted work | `workdeck handoff create/show` | Recover accepted requirements, current source, completed checks, blockers, and pending operations |
| Resolve explicit uncertainty | `workdeck question create/answer` | Preserve a question and its decision without burying it in a transcript |
| Explain completion | `workdeck issue done <ID> --dry-run --json` | See exactly which acceptance, review, or evidence requirement remains unsatisfied |

The proposed top-level `next` is an action recommendation over current task state. Existing
`issue next` remains work selection. Both use the same state/policy model and provide reasons.
Returned action suggestions are typed operations with preconditions, not shell snippets to
execute blindly. An empty candidate list is a valid answer.

`capabilities` must distinguish installed, configured, reachable, and authorized capabilities.
An unavailable provider is not an invitation to silently fall back to a weaker executor.
Feature flags and protocol versions appear in the response so different agent integrations can
negotiate compatibility instead of parsing error messages.

### Context that survives a fresh agent

A repository context packet should contain:

- Repository identity, accepted source revision, working-tree snapshot identity where applicable,
  issue identity/revision, and acceptance-contract digest.
- Scope and non-goals, relevant feature IDs, design decisions, and requirement sources.
- Relevant files and symbols, scoped repository instructions, and dependency/ownership boundaries.
- Applicable named commands, toolchain/environment prerequisites, check profiles, and expected outputs.
- Current claim/execution references, completed versus stale evidence, active blockers, and questions.
- A small recent handoff, source citations, and a suggested next action with reasons.

Use progressive disclosure: a concise summary with resolvable references first, then expand
specific sections on demand. Include truncation and missing-context information. Keep budgets
explicit; do not dump the backlog or full Feature Tree into every agent prompt.

Packets identify their source snapshot and become stale when relevant requirements or source
inputs change. Distinguish authoritative requirements from quoted logs, imported comments, and
derived summaries. A packet is context, not a new grant of authority.

Workdeck initially assembles deterministic repository/task context. Bowerbird's planned context-
pack compiler can consume that versioned packet and add permitted execution history, environment,
policy, and evidence material. Workdeck should not duplicate Bowerbird's full context compiler or
its private execution-history store.

### Handoffs, questions, and collaboration

Use compact structured handoffs linked to an issue and exact source, not a raw transcript dump.
Preserve what was attempted, verified, left uncertain, and pending reconciliation. A different
model or human should be able to continue without relying on the previous conversation.

Questions have stable identities, the requirement/revision they concern, affected work, an
answer/decision reference, and resolved or superseded state. New answers can invalidate a plan
or acceptance contract. Scope meaningful questions to genuine missing information; do not ask for
confirmation on every routine operation that is already authorized.

Surface potential parallel-work conflicts from linked paths, symbols, issue dependencies, and
active claims. These are advisory unless backed by an explicit enforced resource claim. Let
agents propose smaller independently verifiable issues and handoffs; do not automatically split
work, spawn agents, or claim that static path ownership proves conflict freedom.

### Stable and efficient tool contracts

Add consistent `--json`, `--no-input`, field selection, pagination, bounded output, expected
revision, and request identity where applicable. Every error should expose a stable code,
retryability, changed precondition, and permitted recovery actions. Keep useful failure details
separate from large referenced logs.

Long-running operations return a handle promptly. Waiting on a handle is different from starting
another operation. Support cursor-based event following, reconnect, cancellation requests, and
reconciliation through the owning runtime. A client timeout must not imply that a remote job was
canceled or should be restarted.

Generate command/schema references and an agent workflow skill from the typed catalogs, following
the existing [Workdeck agent-workflow approach](agent-workflows.md). Avoid hand-maintained CLI,
MCP, and documentation variants that disagree. MCP can remain a later transport over the same
operations; its tool model already accommodates input/output schemas and structured results.
See the [MCP tools specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).

## 18. First-class commands, checks, and verification profiles

Command discovery and executable verification are core AX features, not optional snippets in a
README. Extend the planned layout with:

```text
.workdeck/
├── commands/<ID>.yml             # Named repository command recipes
├── checks/<ID>.yml               # Expected verification behavior/result contract
├── check-profiles/<ID>.yml       # Required checks for a purpose
├── questions/<ID>.md             # Scoped questions and recorded decisions
└── issues/<ID>/handoffs/<ID>.md   # Compact task continuity records
```

### Three separate concepts

| Concept | Example | Ownership |
| --- | --- | --- |
| Command | Run the repository's architecture validator | Versioned Workdeck repository recipe |
| Check | Architecture validation passes for a specified source and toolchain | Versioned check requirement plus execution evidence |
| Verification profile | Required architecture, unit, integration, and documentation checks for a change | Workdeck acceptance requirements and selection policy |
| Pipeline / release program | Provision environment, execute jobs, collect artifacts, retry, publish | Bowerbird or another explicitly configured executor |

A command can perform setup/build/test/lint/format/docs work. A check gives its result meaning.
A profile identifies required checks and their acceptance use; it must not grow a parallel
workflow language with scheduling, loops, provider effects, or durable retries.

Command definitions declare identity, description, argument schema, argv or an explicit script
entrypoint, working directory, environment/toolchain needs, timeout, expected artifacts, and
declared effects. Use argv execution by default; shell execution must be an explicit recipe type.
Definitions store secret names/references only, never values.

Illustrative proposed command schema, not a supported configuration yet:

```yaml
schema_version: 1
id: architecture
description: Validate Workdeck crate and source ownership boundaries
execution:
  kind: process
  argv: [cargo, xtask, architecture, check]
  cwd: .
  timeout_seconds: 600
toolchain: repository
effects: [workspace_write]
```

Declared effects are metadata for planning and admission; a manifest cannot grant itself permission
or prove what an arbitrary script will do. Reading a command definition does not execute it.
Workdeck can run explicitly requested bounded foreground local checks without owning an agent
session or durable job scheduler. Longer, remote, isolated, or privileged work uses an executor.

Typical profiles are `quick`, `issue`, `pre-submit`, and `release`. A quick pass is explicitly
partial evidence and cannot substitute for the release profile. Test selection must explain its
coverage basis and conservatively broaden when dependency/impact information is incomplete.

### Verification planning and result contract

Proposed surfaces:

```text
workdeck command list|show|run
workdeck check list|show
workdeck check plan --issue <ID> --profile issue --json
workdeck check run --plan <PLAN-ID> --json
workdeck check status|results|explain
```

A check plan binds the issue/acceptance revision, source snapshot, selected check-definition
digests, toolchain/environment requirements, arguments, dependency inputs, required result
contracts, and applicable policy. Resolve branch names to exact subjects before execution.

Local feedback may evaluate a dirty worktree, but its receipt must identify that content rather
than claiming to have verified `HEAD`. Qualification uses the admitted immutable source subject.
An input change invalidates or requires replanning of the affected evidence. Cache reuse requires
matching the relevant source, definitions, toolchain, environment, and policy inputs; a previous
green badge or matching file modification time is insufficient.

Results distinguish `passed`, `failed`, `skipped`, `blocked`, `not_run`, `canceled`, `stale`, and
`unknown`. Structured report adapters expose test/check identity, actual discovery/counts where
applicable, failure locations, selected excerpts, and artifact references. Exit zero alone must
not prove that an expected suite ran. Unsupported report formats remain visible limitations.

Track the base/accepted check contract separately from proposed test changes. When an issue
requires red/green proof, bind the accepted red checkpoint and subsequent green evidence. A
candidate cannot obtain qualification by skipping a check, deleting acceptance criteria, or
changing its own trusted evaluation harness. Valid test-definition changes remain possible
through the appropriate review process.

Reuse existing attestation formats where appropriate. SLSA build provenance describes how an
artifact traces back to its build inputs; it does not itself establish issue acceptance or
product correctness. Workdeck can reference/verify compatible attestations without inventing a
competing artifact-provenance format. See [SLSA provenance](https://slsa.dev/spec/v1.2/provenance).

## 19. CI/CD commands and the Workdeck–Bowerbird bridge

First-class Workdeck CI/CD commands should be contextual interfaces to execution contracts.
Workdeck supplies the issue, source, and required checks. Bowerbird admits and executes pipelines
and releases, keeps durable state, and returns receipts/evidence. The interface must show which
executor is active and what it can actually do.

| Workdeck surface, proposed | User/agent purpose | Bowerbird contract, planned |
| --- | --- | --- |
| `workdeck ci plan --issue <ID>` | Resolve exact source, required profile, executor, and prerequisites | Pipeline validation and admission planning |
| `workdeck ci run --plan <ID>` | Submit one idempotent qualification request | `bwb pipeline run` / durable Run |
| `workdeck ci status/results` | Inspect running/finished checks and issue evidence coverage | Pipeline/Run inspection and evidence queries |
| `workdeck ci watch` | Follow a resumable result stream | Run/pipeline events and watch |
| `workdeck cd plan --issue <ID> --environment <ENV>` | Resolve qualified artifacts, environment, prerequisites, and policy | `bwb release plan` |
| `workdeck cd apply --plan <ID>` | Apply the exact accepted release plan | `bwb release apply` |
| `workdeck cd status` | Observe delivery and current environment evidence | Release/deployment inspection |
| `workdeck cd rollback --plan <ID>` | Apply a separately resolved rollback operation | Release/deployment rollback |
| `workdeck operation reconcile <ID>` | Resolve an uncertain dispatch or external effect | `bwb operation inspect/reconcile` |

These are design mappings, not executable forwarding strings or currently available Bowerbird
commands. The current Bowerbird CLI is a bootstrap scaffold. The adapter must negotiate actual
capabilities and reject unsupported operations. Local command execution and optional explicit
existing-CI adapters can provide value before Bowerbird's runtime ships.

### One definition of execution

Keep Workdeck command/check requirements in `.workdeck/`. Keep Bowerbird executable workflow and
release programs under Bowerbird's own source layout and compiler contract. Reference pipeline
identities and resolved digests from Workdeck. Do not define the same deploy workflow in two
languages or silently translate arbitrary shell tasks into supposedly equivalent durable steps.

Any adapter/materialization records its mapping version and fidelity. Unknown or unsupported
behavior is explicit; it cannot be labeled qualified through a compatibility wrapper.

### Bridge identities and outcomes

Each dispatch should bind:

- Repository and issue IDs, issue revision, acceptance-contract identity, and claim token where relevant.
- Exact source/candidate identity, verification-plan identity, and executor/program version.
- Expected environment/capability requirements and the applicable authority/budget references.
- Idempotency request identity, operation receipt, and resulting Run identity.

Reconcile retried requests through this binding. One issue can have several attempts and Runs;
resuming a Run does not create a new logical task. Keep proposed, accepted/running, completed,
failed, and uncertain outcomes distinct. Local Git publication and remote runtime admission do
not form one atomic transaction; failures between them need inspectable recovery state.

A successful build can support implementation evidence. A required clean qualification can
support integration/qualification. A deployment receipt and health observation can support
delivery. None automatically establishes business acceptance or a feature's entire maturity.
Promotion evaluates the applicable policy and produces a reviewed feature change where required.

### Release plans and feedback

A release plan binds exact artifacts/source, pipeline, target environment, configuration,
required observations, authority, and expiry. Applying a stale plan must fail rather than quietly
deploy a newly resolved subject. Existing authorization should be reusable within its scope;
changing the subject, target, or authority cannot be hidden behind a generic `--yes`.

Rollback is a new bounded operation with its own prerequisites and possible failure/ambiguity.
Do not assume an application rollback reverses data migrations or external effects. Bowerbird
owns those execution semantics; Workdeck shows the plan and evidence in task context.

After delivery, link relevant health/regression observations to the delivered subject. A confirmed
regression can propose a deduplicated follow-up issue with reproduction evidence and scope. Policy
decides whether it may be claimed automatically. Avoid uncontrolled loops that invent work,
consume budget, or repeatedly repair the same uncertain external outcome.

## 20. AX implementation priorities and success measures

The current [standalone plan](project-management-implementation-plan.md) schedules AX work across
PM-07 (context, next actions, questions, handoffs), PM-08 (commands and verification), PM-09 (Git
coordination), and PM-11 (CI and completion evidence). Schema and structured-output foundations
begin in PM-00–PM-02. External runtime dispatch and delivery remain future integration work.

Start with **context packets, a named command catalog, verification profiles, and structured
results**. These improve every agent interaction even before durable CI/CD integration exists.
Define bridge interfaces early; add implementations when the runtime contracts can be exercised.
Do not block basic Workdeck utility on an unimplemented Bowerbird service.

Measure AX through behavior rather than the number of commands or issues closed:

- Time and tool calls from a fresh agent to the first valid task action.
- Context size and unnecessary source rereads, alongside missing-context failures.
- Rate of successful handoff/resume without repeating completed work.
- Wrong-command, wrong-repository, stale-evidence, and duplicate-dispatch failures.
- Accuracy of readiness/completion explanations and check-selection coverage.
- Cost/time to obtain accepted evidence and independently reviewed outcomes.
- Reopened work, escaped regressions, and the proportion of uncertainty resolved explicitly.

Use fresh-session exercises in temporary repositories: give a new agent an issue ID, allow it to
discover the required context and commands, interrupt it, resume with another agent, and verify
that source/evidence/claims remain correct. The release pilot then tests the same contracts with
real executor receipts and observed outcomes. This is the practical acceptance test for the
engineering operating-system direction.
