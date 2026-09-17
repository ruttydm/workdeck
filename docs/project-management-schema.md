# Workdeck PM schema and authority contracts

Status: implementation in progress. The
[implementation plan](project-management-implementation-plan.md) tracks qualification
and remaining scope.

The typed records live in [model.rs](../crates/workdeck-pm/src/model.rs), identity
contracts in [identity.rs](../crates/workdeck-pm/src/identity.rs), and workflow
contracts in [workflow.rs](../crates/workdeck-pm/src/workflow.rs). Call
`Config::validate()` and `IssueMetadata::validate(&config)` after deserialization
and before writing; serde shape checks alone do not evaluate source configuration.
The document layer separately preserves Markdown and source formatting.

## Configuration and identity

The versioned PM configuration is `.workdeck/config.yml`. Existing application
preferences use TOML and remain a separate configuration contract; YAML is not a
replacement parser for theme, keybinding, review, or extension preferences.

```yaml
schema: 1
repository: repo-01ARZ3NDEKTSV4RRFFQ69G5FAV
prefix: WD
acceptance:
  require_all_criteria: true
  require_description: false
```

`schema` currently accepts only integer `1`. `repository` is an immutable
`repo-<full ULID>` identity independent of clone paths or Git remotes. New issue
IDs use a configured prefix of 1–12 uppercase ASCII letters and a full canonical
ULID. Existing positive decimal `WD-N` identities remain parseable for migration.
IDs do not encode title, workflow status, or directory hierarchy. Changing a
configuration prefix does not rewrite existing identities.

A repository-qualified reference is `repo-<ULID>::<record-ID>`. A prefix or local
path alone cannot disambiguate repositories. Revisions are positive `u64`
integers; `Revision::next()` rejects exhaustion rather than wrapping. A
`SourceToken` combines the stored revision and the SHA-256 identity of the exact
source bytes, so a manual edit without a revision increment remains detectable.

`Config::new(prefix)` generates the repository identity, default workflow and
acceptance policy, then validates the result. Optional extension data goes under
`custom:` or a top-level `x-` name. Unknown unnamespaced fields are errors.

## Workflow and minimal issue

The default initial state is `ready`, with medium priority. This deliberately maps
the prototype's `todo` default to a stable workflow ID; compatibility aliases
belong to command adapters, not authoritative stored status values.

| State ID | Default display name | Semantic category |
| --- | --- | --- |
| `inbox` | Inbox | `triage` |
| `backlog` | Backlog | `backlog` |
| `ready` | Ready | `unstarted` |
| `in_progress` | In progress | `started` |
| `in_review` | In review | `review` |
| `verification` | Verification | `verification` |
| `done` | Done | `completed` |
| `canceled` | Canceled | `canceled` |

Configured states require unique stable IDs, nonempty display names, existing
transition targets and an existing nonterminal initial state. Terminal states
have no ordinary outgoing transitions; reopening is an explicit application
operation. A transition into a completed category still requires completion
policy. Merely validating a workflow edge cannot establish acceptance.

Each issue's structured metadata belongs in
`.workdeck/issues/<ID>/item.md`, followed by a Markdown body:

```markdown
---
schema: 1
id: WD-01ARZ3NDEKTSV4RRFFQ69G5FAV
revision: 1
title: Preserve the login destination
status: ready
priority: medium
created_at: 2026-09-08T10:00:00Z
updated_at: 2026-09-08T10:00:00Z
files:
  - path: src/auth.rs
    line: 12
    end_line: 24
documents:
  - docs/authentication.md
acceptance:
  - id: login-deep-link
    description: Deep links survive login
    checked: false
custom:
  risk: medium
x-import:
  source: legacy
---
Preserve the requested destination through the login callback.
```

`IssueMetadata::new(config, title, now)` uses the configured initial workflow
state, a new immutable issue ID, revision 1, medium priority, equal created/updated
instants and empty optional data. Construction trims the title. Validation rejects
empty titles, control characters, unknown states and inconsistent timestamps.

| Field group | Schema-1 validation |
| --- | --- |
| `schema`, `id`, `revision` | Supported schema, canonical ID, positive revision |
| `title`, `status` | Nonempty single-line title; an actual configured state ID |
| `priority` | `none`, `low`, `medium`, `high`, `urgent`; medium when omitted |
| `created_at`, `updated_at` | RFC 3339 instants with an offset, represented in UTC; update cannot predate creation |
| `assignee`, `reporter`, `reviewer` | Optional single-line attribution; registered identity mode requires active IDs on new/changed attribution. This is not authorization. |
| `due_at` | Optional exact `YYYY-MM-DD` date or RFC 3339 instant with an explicit offset; no timezone guessing |
| `completed_at`, `canceled_at` | Correspond to the terminal workflow category and fall within creation/update bounds; an active historical import explicitly permits an unknown completion time |
| `archived` | Boolean, false when omitted; archival is independent of workflow status |
| `project`, `cycle`, `milestone`, `targets` | New/changed associations require canonical nonretired records; milestones belong to the associated project. Archived references remain valid. Historical unresolved declarations are diagnosed. |
| `parent`, `prerequisites` | Canonical child/dependent-owned issue edges; new references resolve in the transaction snapshot and cycles are rejected. |
| `features`, `gates` | Canonical typed native IDs; new/changed associations require existing nonretired records. |
| `labels`, `commits`, `documents` | Unique nonempty single-line references; no external resolution or command execution |
| `files` | Portable repository-relative links and valid optional one-based ranges |
| `acceptance` | Unique stable lowercase criterion IDs, nonempty descriptions, explicit checked state |
| `manual_acceptance` | Optional actor, reason and accepted instant within creation/update bounds; explicitly manual attribution |
| `imported_completion` | Source path/hash and import instant for historical Done without a known completion time; reopening records supersession while retaining provenance |
| `estimate` | Exact nonnegative decimal string plus unit; reports group by unit, never combine incompatible units. |
| `custom`, top-level `x-*` | Extension values preserved; active `schema.yml` declarations validate their scoped custom keys. Extension fields do not acquire evidence semantics. |

Commit and document references remain inert data. A nonempty commit reference is
not proof that a Git object exists or that a check ran against it. Association
resolution, referenced-object deletion policies, and exact evidence subject
resolution are subsequent application contracts.

Source paths use `/` separators and exclude absolute roots, empty components,
`.`/`..`, backslashes, control characters, Windows-reserved names/characters, and
trailing dots/spaces. A range starts at line 1 or later; `end_line` requires
`line` and cannot precede it. `SourceLink::validate()` is purely lexical and never
reads or follows a path. Filesystem adapters must separately establish root
containment and inspect symlinks before resolving a link for an actual operation.
A lexically valid link can deliberately point to a file absent from this clone.

## Extensions and reserved fields

The schema accepts arbitrary JSON-compatible extension values beneath `custom:`
and stable top-level `x-` names, such as `x-import`. Typed serialization preserves
those values; preserving comments, ordering and exact body formatting belongs to
the document layer. The parser must reject duplicate mapping keys before typed
validation rather than allow a typo or duplicate to replace a known field.

Unknown unnamespaced keys such as `titel` and `workfow` produce `invalid_schema`.
Recognized later-phase issue fields currently produce `unsupported`: `type`,
`related`, `evidence`,
`target`, `claim`, `review_session`, and `last_activity_at`. Config reserves
`sources`, `coordination`, `custom_fields`, `claims`, and `providers`.
These names cannot quietly become ignored requirements. Later phase implementations
must add explicit typed behavior and tests when enabling them.

The distinction is intentional: a declaration under `custom` or `x-*` is inert
metadata. Putting `features` into `custom` does not activate graph validation or
activate native feature associations. PM-05 implements project/cycle/milestone/
target records and reference policies; PM-06 implements typed `features` and `gates`
associations separately from inert custom data. Custom-field declarations add typed validation
for explicitly named keys; they do not promote arbitrary extension data to requirements
or verification evidence.

## Acceptance declarations and completion

`AcceptancePolicy` defaults to `require_all_criteria: true` and
`require_description: false`. A criterion has a stable lowercase slug ID, a real
single-line description and a boolean `checked` value. Duplicate IDs are invalid.
The model validates declarations; it does not infer that a Markdown checkbox or
an imported claim is verified evidence.

Future check/profile requirements are preserved as explicit declarations:

```yaml
acceptance:
  require_all_criteria: true
  require_description: true
  required_checks: [auth-tests]
  required_profiles: [issue]
```

Configuration validation permits well-formed unique slug references here.
`AcceptancePolicy::ensure_supported_for_completion()` must reject a policy with
required checks or profiles until their real evidence evaluators exist. Completion
callers invoke this guard before evaluating baseline criteria, and must separately
evaluate checked criteria and the Markdown description. Allowing a configuration
to load never turns these future declarations into passing checks.

The [minimal fixture](../crates/workdeck-pm/tests/fixtures/minimal-repo/README.md)
intentionally contains such references: it is a valid source contract with
explicitly unsupported completion requirements, not a check-runner demo.
Local feedback, manual acceptance, reviewed evidence and trusted CI qualification
will remain distinct as the later policies are implemented.

`ManualAcceptance` records `actor`, `reason` and `accepted_at`; actor and reason
must be nonempty single-line text. Its timestamp is bound to the issue's creation
and update interval. Baseline completion need not invent a manual acceptance
record, and manual acceptance is never a trusted CI result. Application completion
reports distinguish `basis: declared` from `basis: manual`; neither denotes local
verification or CI qualification. Reopening clears prior acceptance so it cannot
silently qualify changed work.

Legacy Done records may lack a completion timestamp. `ImportedCompletion` retains
the source path, exact source hash, and actual import instant without substituting
the issue's update time for its unknown completion time. Such reports use
`basis: imported`; this is historical attribution, not verified evidence.
Reopening sets `superseded_at` and retains the original provenance. A subsequent
native completion records its actual time. Normal create/update/editor operations
cannot forge or replace this managed provenance.

## Metadata authority

`MetadataAuthority` classifies roles without permitting those roles inside issue
frontmatter. `for_issue_field()` recognizes both currently stored issue fields
and named derived/local/coordination/reference concepts so consumers can make the
boundary explicit.

| Authority | Examples | Source and mutation contract |
| --- | --- | --- |
| `authoritative` | Issue requirements, workflow status, stable IDs, versioned configuration, extension metadata | `.workdeck/` planning source; validated writes with revision/content preconditions |
| `derived` | Reverse prerequisites, children, `last_activity_at`, blocker explanations | Computed from the identified source; not copied into issue frontmatter as competing truth |
| `local` | Selection, scroll position, drafts, per-machine preferences | Explicit local application state or ignored `.workdeck/.local/`; not shared requirements |
| `disposable` | SQLite/FTS rows, search scores, cache data | Ignored `.workdeck/.index/`; deletion cannot lose authoritative records |
| `coordination` | Claim actor/token/generation/expiry | Separate claim record; local coordination differs from confirmed publication on a configured coordination ref |
| `review_reference` | A link to a review session or revision-bound review conclusion | Review subsystem owns the session; the issue stores an explicit reference only when implemented |

In shared mode, reviewed planning content belongs to the accepted ref; an
implementation branch holds proposals, and a coordination ref holds claims.
Those publication semantics are later-phase requirements, not guarantees of this
schema library. A claim is neither a review session nor an agent process lease.
Author names/emails provide attribution and are not authorization.

## JSON mutation adapter contract — API v1

`workdeck init` and the retained `workdeck --init` flag both initialize native
`.workdeck/config.yml`. They preserve existing `.workdeck/config.toml` preferences
and do not create the old `.agents/workdeck` scaffold. Application preference defaults remain available
in memory; initialization does not need to overwrite a preference file.
Legacy planning data requires explicit migration rather than an implicit import.

Application preferences use `.workdeck/config.toml`; `workdeck config init` creates
only those preferences, and does not initialize PM. A sole existing legacy app
configuration remains selected for compatibility until explicit migration. Config
mutations validate all effective layers before publication, preserving TOML comments
and inline tables. Fresh issue/reference commands require `workdeck init`; actual
legacy sources retain their compatibility readers until migration.

The native adapter emits `api_version: 1`, `ok`, `kind`, `source`, and `result`;
successful mutations also include their durable `receipt`. Errors replace the
result with the structured error contract. Legacy sources retain the older JSON
envelope until explicit migration, as recorded in the
[compatibility inventory](project-management-compatibility.md).

Native `issue`, `project`, `cycle` and `label` deletion requires `--yes` and retains
the authored record plus a permanent `.workdeck/tombstones/<kind>/<ID>.yml` marker.
`delete ID --dry-run --json` returns the canonical target, source token, incoming
references and a fingerprint; `--expected-preview HASH` checks that reviewed source
and reference membership inside the mutation. Issue abbreviations resolve after
durable request replay. Existing expected revision/content and explicit staging flags
also apply. Retired records remain readable with `retirement` metadata, but writes,
restoration and ID reuse are rejected. Retirement preserves history and does not
establish check or CI evidence.

For project/cycle/label deletion, `--force --dry-run` produces a concrete association
resolution plan. Each affected row identifies the issue, source token, field, before
and after values, and preserved history. Apply with `--force --yes --expected-preview
HASH`; a force flag alone does not authorize clearing an unreviewed set of references.
The shared operation rechecks membership, configuration, content and history under
the writer lock, then clears only the reviewed associations and retires the reference
in one recoverable change set. Normal issue policy applies: accepted work that cannot
be edited must be reopened explicitly before a new plan is reviewed. The original
composite receipt survives replay after later issue edits, and `--stage` scopes the
complete change set while preserving unrelated index entries.

Native `agent` commands retain recorded session annotations under
`.workdeck/imported-sessions/`; they never host a process. Their JSON result identifies
`historical_annotation` and a content hash. Mutations accept `--request-id`, optional
`--expected-content`, and explicit `--stage`. `agent import` accepts a bounded JSON
object/array or JSONL and applies the entire batch transactionally. Native deletion
retains history and reserves the identity. `events list` keeps imported historical
events separate from durable planning mutation receipts; neither proves CI qualification.
Deleted sessions retain their TOML and a marker bound to the deletion receipt's
canonical returned-record digest. Missing markers, forged receipt results and
case-alias reuse are diagnostics. Unknown nested touched-file metadata and comments
survive semantic edits. Ordinary retries return the original receipt even after
later edits or retirement. These editable repository records provide consistency
checks, not signatures or external authentication.

`Snapshot::list_bounded` enforces traversal limits before collecting the whole tree.
Files and directories below the selected prefix count toward the limit; ignored
local state does not. Cached reads retain the strictest successful limit. New journals
persist that limit, and recovery reserves bounded space for the operation's own
new files and parent directories. Older journals without this field default to the
engine ceiling of 1,000,000 entries. History uses tighter 10,000-entry prefix limits
and 64 MiB aggregate content limits; per-session documents remain bounded at 2 MiB.

`export`, `export --json`, and `export --jsonl` preserve a versioned native snapshot
of exact supported PM bytes, including receipts and migration provenance. The JSONL
stream has one snapshot header with file count followed by typed file frames; unknown,
duplicate, malformed and truncated records are errors. Current bounds are 32 MiB of
content, 48 MiB of input and 4,096 files, with no silent omission at the limits.

`import PATH --dry-run --json` validates the source and projected destination and
returns a reviewed-plan fingerprint. Applying with `--expected-plan HASH` and
`--request-id ID` checks that plan inside the transaction. `--merge` creates missing
records and permits identical records; native `--replace` replaces eligible matching
active records while retaining unrelated records and immutable history. Current import
requires the same repository identity and already matching config/operation/migration
authority. Restoring missing authority uses the explicit restoration protocol below;
the same-authority merge path is not a general backup restore.

Legacy bare JSON, successful export envelopes and typed JSONL can be imported into
an initialized native source. The importer retains exact original bytes at
`imported-history/exports/<SHA-256>.json` or `.jsonl`, reuses the migration field/status
mapping and preserves historical completion provenance. Native records retain real
origin paths and row selectors; an import does not establish verification evidence.
`--replace` cannot overwrite matching native history with older legacy revisions.
Unknown kinds/collections, duplicate JSON keys and invalid framing are errors.

Legacy preview returns a concrete `imported_at`; apply a reviewed plan with both
`--expected-plan HASH` and `--imported-at TIMESTAMP`. An ordinary import can omit the
timestamp; it is then selected inside the replay-aware mutation. Reusing the same
request returns the original receipt after later changes. JSON-only values such as
null and large unsigned integers retain their type under the native legacy metadata
namespace or explicit tagged session encoding, backed by the exact original artifact.
Repeated unchanged imports reuse the original validated import time; changed native
records still conflict. Legacy JSONL predates counted framing, so missing complete
trailing records cannot be detected; previews report this limitation. Native snapshot
JSONL includes a declared file count. Focused implementation evidence is recorded in
the [validation record](project-management-validation.md#cutover-work-in-progress--2026-09-09).

`import PATH --restore --dry-run --json` previews restoration into a fresh or
partially populated root. `--restore` preserves the snapshot's repository identity
and original receipts and permits only missing or byte-identical authority.
Conflicting files are blockers. The reviewed fingerprint includes preserved app
preferences and destination membership. Apply with `--expected-plan HASH` and
`--request-id ID`; optional `--stage` includes the complete restored manifest and
original receipt paths while preserving unrelated index entries.

Restoration stages its bounded input under ignored `.tmp/restorations/` and publishes
`restore.yml` before authoritative writes. Normal reads, initialization and writers
refuse a pending restoration, including before config exists. Resume explicitly with
`import --restore --resume --request-id ID`; the original external input is unnecessary.
Preconditions preserve external edits, and the barrier remains until restored content
and its durable receipt have been synced and validated. Replays return the original
receipt; stale bytes still prevent later staging. The protocol has focused CLI and
fault-test evidence and remains under final integration review.

CLI automation provides request identity and optional expected revision **and**
content hash as flags. For example, after reading an issue's source token:

```sh
workdeck issue --request-id fix-login-1 \
  --expected-revision 1 \
  --expected-content aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  update WD-01ARZ3NDEKTSV4RRFFQ69G5FAV \
  --title 'Preserve the original destination through login' --json
```

The hash above illustrates a lowercase 64-character SHA-256 representation; a
real caller uses the exact identity returned for the bytes it read. Checking only
the revision cannot detect a direct editor change. Request IDs accept bounded
ASCII alphanumerics, `_` and `-`; operation IDs are separately generated `OP-<ULID>`
identities. Repeating the same request must return its original durable result;
reusing an identity with different inputs must fail, including after cache deletion.

Omitting the expected token applies a semantic intent to the current locked
snapshot; it does not authorize an unconditional byte overwrite. Replay lookup
precedes record resolution, new IDs, timestamps, and workflow decisions. Editor
drafts always capture an expected source token. Deterministic editor retries use
`issue edit --from-file`; interactive editors use a new request and retain failed
drafts with an actionable path.

Native receipts bind schema, repository identity, operation/request identity,
input hash, original result, and changed paths with before/after hashes. The
repository identity is pinned when opened and checked under the operation lock,
including before replay. These receipts establish local mutations; publication
and remote reconciliation remain separate later-phase work.

`workdeck operation pending --json` and `operation recover --dry-run --json`
inspect interrupted durable intent without applying changes. `operation recover`
explicitly finishes only a change set whose recorded preconditions still hold.
Conflicting direct edits remain untouched. `--source PATH` can explicitly select
the planning root when discovery cannot choose a source.

Errors have stable `code` plus a useful `message`; optional `path`, `line`,
`column` and `hint` identify actionable context. `ErrorCode::exit_code()` and
`retryable()` give consistent machine behavior. Retryability permits reloading
and reevaluating changed preconditions, never blindly applying an old write.
Human errors include escaped paths and line/column. Editor output goes to stderr
so JSON stdout remains parseable.

`--stage` is explicit on native issue, project, cycle, and label mutations. It
stages only the exact paths changed by that receipt and the durable receipt
itself. Other staged and unstaged work is preserved; Git filters and hooks do not
run. Staging never commits. The successful JSON envelope adds a `staging` report.
A staging failure after the file mutation returns a nonzero error with
`error.details.mutation_committed: true` and `error.details.receipt`. Retrying
the same request replays the original mutation before attempting staging again.
Changed source bytes and unrelated staged changes to an affected path prevent
staging. Staging currently requires the qualified Unix Git-index implementation.

Reference commands use the same envelope and request/source flags. `project`,
`cycle`, and `label` expose `create`, `update`, `list`, `show`, `save`, and
`archive [--restore]`. `create` allocates a full kind-prefixed ULID unless an ID
is supplied. Compatibility `save NAME` keeps the existing ASCII slug convention,
and performs create-or-update inside one replay-aware transaction. An omitted
description preserves the body; an explicitly empty description clears it.
Deletion uses the reviewed retirement operation described above. Archive retains
identity and associations. Native reference commands never fall through to legacy writers.

`workdeck capabilities --json` reports the selected source's availability, actual
Clap command definitions, schema names, and current feature availability without
initializing an empty repository. Use `capabilities --fields source,features,command_version
--compact --no-input` for a small startup response instead of the full command catalog.
`workdeck schema --json` lists schemas;
`workdeck schema issue --json` returns the generated schema for issue metadata.
Schemas derive from the Rust types using [Schemars](https://docs.rs/schemars/1.2.2/schemars/).
Authoring schemas use the same writable-field list as mutation validation.
JSON Schema describes data shape; `doctor` and application operations additionally
enforce document syntax, workflow, reference, and completion semantics.

## Explicit prototype migration

Migration lives under the existing command family and defaults to read-only
preview. Save and review its exact proposal before applying it:

```sh
workdeck migrate legacy --plan-out migration-plan.json --json
workdeck migrate legacy --apply --plan migration-plan.json \
  --request-id migrate-backlog-1 --json
workdeck migrate legacy --resume --request-id migrate-backlog-1 --json
```

`--source` and `--destination` select explicit roots for a preview; defaults are
the repository's `.agents/workdeck` and `.workdeck`. Application is bound to the
current repository destination unless that exact alternate destination is
selected explicitly. A plan file is created only with `--plan-out`, must be
outside both roots, and never replaces an existing file. `--plan -` accepts a
saved bare proposal or successful API v1 preview envelope on stdin. Preview
success means inspection succeeded: callers must inspect `complete` and
`blockers`; missing historical dates are not invented.

Apply and resume require a stable request ID. A pending migration prevents normal
PM access until the same operation is recovered. Cutover verifies its plan,
manifest, receipts, and repository identity; subsequent native edits do not
invalidate the completed migration or cause a retry to overwrite them. Legacy
source files remain available. Application-wide app config, extension, export,
import, and legacy-view cutover is still being integrated; this migration command
alone does not certify that every historical consumer has moved.

## Validation boundary

[Schema acceptance tests](../crates/workdeck-pm/tests/schema.rs) exercise default
construction, configured workflow, timestamp consistency, custom/extension
round trips, typo/future-schema diagnostics, portable source links, acceptance
identity, unsupported future-policy completion, due dates, and authority roles.
The malformed and custom-field fixtures are isolated synthetic inputs.

These tests establish model contracts. They do not establish safe file writes,
repository discovery, Git claims, CLI/TUI integration, check execution, trust
policy, or release readiness. Those requirements retain separate evidence rows
in the implementation plan.

## Plain Markdown wiki authoring (PM-05 increment)

Wiki documents live at `.workdeck/wiki/<path>.md`. They are plain UTF-8 Markdown,
without required frontmatter or an embedded schema. Workdeck preserves exact body
bytes, including line endings, and never executes scripts or follows links during
inspection. Existing documentation elsewhere in the repository stays where it is;
issue document references can point to those files or wiki paths.

Wiki path components use lowercase portable slugs; the final component ends in
`.md`. Paths are limited to 512 bytes and 16 components. A document is at most
2 MiB, must be UTF-8, and cannot contain NUL. Listing/doctor traversal is bounded
at 10,000 entries and 32 MiB of body content; unsupported input fails explicitly.

`WriteWiki.expected = null` is create-only. An update requires the previously read
SHA-256 `content_hash`. The shared transaction engine pins source content and
membership, publishes the document and durable receipt recoverably, and replays
the original request without reinterpreting a later edit. Wiki files participate
in native snapshots, reviewed restore, and explicit replace-matching import.

The CLI adapter exposes `wiki list`, `wiki show <path>`, `wiki create <path>
--body-file <file|->`, and `wiki update <path> --expected-content <sha256>
--body-file <file|->`. Authoring also accepts `--body`; `--request-id`, `--json`,
and exact scoped `--stage` are available. CLI integration is undergoing validation;
this section does not claim the remaining PM-05 organization/TUI work is complete.


## Planning hierarchy and membership (PM-05 qualified)

Initiatives, projects, milestones, cycles and targets use
`.workdeck/<plural>/<immutable-slug>/item.md`; labels retain their aggregate
`labels.yml` representation. Projects can reference an initiative. A milestone
must reference its project. Projects expose lead, scope, goal, dates and exit
criteria; initiatives and milestones expose outcome declarations. None of these
fields imply that an evaluator verified an outcome.

Issues may reference one project, cycle and milestone and multiple targets. A
milestone association must agree with the issue's project. Project/milestone
`targets` contribute inherited issue membership. Target membership is derived
from these canonical outgoing references; there is no second editable members list.
New/changed associations require exact canonical, nonretired identities. Archived
records remain referenceable. Historical unresolved or case-alias references are
readable with explicit diagnostics; unrelated edits do not silently rewrite them.
Retirement of a referenced record requires the available reviewed resolution plan;
archiving preserves associations. Unresolved incoming planning links block retirement.

`planning_membership` captures its selected record, related planning records and
filtered issues with source/config identities under one snapshot. The CLI exposes
it with `<kind> show ID --members --archive active|all|archived`; all scalar query
predicates and target matching use `IssueQuery`. The terminal's F11 Projects and
F12 Cycles views use these same operations and keep draft text and original source
preconditions. Ctrl-D explicitly discards a planning draft. A retained request does
not become a new operation merely because its text changes after a lost acknowledgement.

## Issue time records (PM-05 qualified)

Each `.workdeck/issues/<issue>/time/<TIME-ID>.yml` records its own immutable identity,
repository/issue identity, user and actor, seconds, worked-at instant, recorded-at
instant and captured cycle. Logging time does not change the issue's source revision.
An amendment creates a new record with `supersedes`, the prior entry's expected content
hash and a reason. It retains the original cycle attribution even after reassignment
or retirement. Zero seconds is an explicit correction. Missing predecessors, forks,
cycles and duplicate time identities are rejected; reports count only active chain
leaves and use checked arithmetic.

The CLI provides `time log ISSUE`, `time amend ISSUE TIME-ID`, `time list ISSUE`, and
`time report`. Log/amend take explicit seconds, worked-at timestamp, user and actor;
amend additionally requires the predecessor content hash and reason. Reports filter
by issue/user/cycle and time bounds and show totals by those dimensions. Registered
identity policy applies to new attribution. Native snapshot replacement cannot rewrite
immutable time history; exact historical restore preserves it. Valid direct file edits
remain visible in Git, so this is append-only application behavior, not tamper-proof
accounting or a declaration of billable time.

## Saved issue views (PM-05 qualified)

`.workdeck/views/<slug>.yml` stores `schema`, repository identity, immutable `id`, `name`,
`archived`, and the shared `IssueQuery`. Optional `custom` and `x-` fields remain intact
when the authoring API changes known fields. IDs are portable lowercase slugs. Files are
bounded to 64 KiB and listings to 1,024 definitions. No query executes repository code.

`workdeck view create ID --name NAME --query '{}'` creates only; updates require the exact
`--expected-content` from `view show ID --json`. Use `--query-file PATH` (or `-`) for JSON
input. `view update` replaces the explicit name, predicate and archived flag; pass
`--archived` to archive a definition. Archiving does not change its predicate or its issues.
`view list` includes archived definitions; `view run ID` reads the definition and current
issue/workflow sources under one snapshot, returning the selected issues and definition's
content identity. Query status aliases use the captured workflow. Unknown aliases fail
explicitly during authoring/evaluation; historical stored predicates remain inspectable
for repair after a workflow change. Definition hashes are edit preconditions, not evidence
that the selected issues have completed work. Saved results are never stored as authority.

The shared API provides `saved_view`, `saved_views`, `write_saved_view`, and
`query_saved_view`. Writes support request replay and scoped staging; doctor, export,
import, and fresh restore recognize saved-view authority. Integrated PM-05 qualification
is still pending.


## Organization CLI (PM-05 qualified)

Organization policy lives in optional `.workdeck/users.yml` and `.workdeck/schema.yml`.
Reading absent policy returns open defaults without creating those files. These commands
require an initialized native repository; legacy sources remain read-only. Existing
`workdeck schema NAME --json` continues to inspect generated data schemas.

Declare identities and patch their display details or individual custom values:

```sh
workdeck user create worker "Engineering agent" --kind agent --request-id user-worker --json
workdeck user update worker --name "Review agent" --set 'team="platform"' --json
workdeck user mode registered --json
```

`user list` returns the registry and its aggregate source token; mutation results return
that same record shape. `user show ID` returns the identity definition. User writes accept
paired `--expected-revision` and `--expected-content`. `user archive ID` preserves identity
and history; `--restore` reactivates it. `user update --from-json PATH` explicitly replaces
the complete definition, while ordinary update flags preserve unmentioned metadata.
Registered mode checks identity declarations against the registry; it does not execute agents.

Schema changes replace only the named field/unit definitions. For example, `change.json`:

```json
{
  "fields": {
    "risk": {"type": "enum", "scopes": ["issue", "project"], "options": ["low", "high"]}
  },
  "units": {"hours": {"name": "Hours"}},
  "unit_mode": "registered"
}
```

```sh
workdeck organization schema show --json
workdeck organization schema preview change.json --json
workdeck organization schema apply change.json --expected-preview HASH_FROM_PREVIEW --request-id schema-risk --json
```

Replace `HASH_FROM_PREVIEW` with `result.fingerprint` from the reviewed preview. The hash
binds the proposed policy and current source dependencies; intervening changes require a
fresh preview. Preview reports compliance and blockers without writing. Required fields
must be populated on active records before a tightening change can apply. Identity/type
repurposing is rejected; archive definitions explicitly. User and schema mutations replay
an existing request before recomputing current policy, provided the semantic input matches.

Patch custom keys and declare estimates through the shared issue/planning writers:

```sh
workdeck issue custom "$ISSUE_ID" --set 'risk="low"' --set 'context={"reviewed":true}' --json
workdeck project custom "$PROJECT_ID" --set 'risk="high"' --unset obsolete --json
workdeck issue estimate "$ISSUE_ID" 0.25 --unit hours --json
workdeck issue estimate "$ISSUE_ID" --clear --json
workdeck organization estimate-report --archive active --json
workdeck organization compliance --check --json
```

Custom `--set KEY=JSON` and `--unset KEY` preserve other keys, including nested unknown
values. All six planning kinds expose `custom`; planning creation also accepts an explicit
`--custom JSON_OBJECT` so required values can be supplied immediately. That option replaces
the full custom object on update. Policy validation rejects conflicting patches, missing
required values, and invalid declared types before publication.

Estimate values are exact nonnegative decimal strings with at most six fractional digits.
Reports use the shared issue filters and return separate totals per unit, without unit
conversion or completion inference. The report's default archive scope matches `issue
list` (all); use `--archive active` to exclude archived records. Compliance inspection
returns the report; `--check` exits with `policy_blocked` and includes violations in the
error's `details` when policy is not satisfied. JSON file input is bounded, accepts `-`
for stdin, and rejects special files. Mutations support stable `--request-id` and explicit
scoped `--stage`; none of these commands creates native authority implicitly.

Focused CLI tests cover these contracts. Combined PM-05 qualification is still pending.


## Native work graph, features, gates and evidence (PM-06 integration)

Issue `parent` and `prerequisites` fields own directed edges. Children and dependents
are derived from the complete source snapshot. Symmetric soft links have one sorted
pair document under `relations/issues/`; they do not affect readiness. Reasoned
prerequisite resolution and replacement are semantic operations. Waivers are disabled
by default and, when enabled by `acceptance.allow_prerequisite_waivers`, bind the
requirement, prerequisite source, policy and durable request decision. Canceling a
prerequisite neither removes nor satisfies its requirement. Parent, hard-dependency
and mixed completion cycles are rejected. Direct-editor debt remains visible.

The graph's `satisfied`, `unsatisfied` and `unknown` conditions carry stable reason
codes, related subjects, paths and source pins. `issue relations`, `issue ready` and
`issue dependency-path` use a full captured graph. Readiness concerns hard prerequisites;
completion also enforces required children. No dependency path is a calendar forecast.
Completed prerequisites and children retain unresolved gate conditions; their status
alone cannot erase that debt. Dependency paths expose reachable unresolved references
even when no path to the requested destination can be established.
The issue workbench opens this captured graph with `b`; arrows select, Enter follows,
Backspace returns, and `r` explicitly refreshes. The source remains pinned across errors.

New graph mutation receipts retain the exact request intent and validate its hash,
reported mutation, source preconditions and waiver decision consistently on replay,
history, staging, snapshot transfer and interrupted-journal recovery. Earlier graph
receipts without the additive intent field retain their structural historical policy;
their omitted full inputs cannot be reconstructed. Replaying such a receipt still
checks the supplied exact request against its recorded mutation. These consistency
checks do not authenticate the declared actor.

Native IDs use fixed canonical ULID namespaces: `FEAT-`, `GATE-` and `EVD-`. Feature
files are Markdown at `features/[optional/group/]<FEAT-ID>.md`; physical relocation,
logical reparenting and renaming preserve identity. Feature decisions are
`proposed|accepted|deferred|rejected`, declared maturity is
`draft|specified|implemented`, and availability is
`unavailable|experimental|available|deprecated`. No public field authors verified
maturity. Features can have parent/prerequisite edges, symmetric soft links under
`relations/features/`, targets, project/milestone links, source links, criteria and
gates. `IssueMetadata.features` is the sole forward authority for issue membership;
coverage is derived, and finishing an issue never promotes a feature.

`feature create/show/list/update/parent/move/archive/custom/relate/unrelate/coverage`
exposes the shared operations. `feature delete --dry-run` previews incoming references;
`--yes` performs permanent retirement, optionally bound to `--expected-preview`.
Source documents/history remain retained and the identity stays reserved. Feature/gate
retirement requires full canonical IDs; feature lookup and
ordinary feature mutations also accept unambiguous short references. Feature coverage
retains prerequisites omitted by its issue filter and warns about missing transitive
references. Issue
create/update accepts repeatable `--feature` and `--gate` flags and explicit
`--clear features|gates`. Accepted issue content must be reopened before alteration.
In the issue workbench, `v` opens features; Shift-F11 also opens the native feature
view. Create/edit drafts retain invalid input and their original source. `a` archives
or restores and `x` toggles active/all; `r` refreshes derived coverage.

Gates are versioned YAML at `gates/<GATE-ID>.yml`. Their nonempty requirement list is
flat AND. Every requirement identifies a stable criterion, producer/check identities
and definition hashes, with an optional maximum evidence age. `gate criterion
OWNER_KIND OWNER_ID CRITERION_ID` resolves the exact semantic criterion reference.
Gate `update`, `custom` and `archive` require both inspected revision and content
hash; `delete` uses the shared retirement preview/operation. Generated IDs and times
are allocated inside replay-first transactions.

Evidence has one canonical namespace at `evidence/<EVD-ID>.yml`, refining the older
illustrative issue-local layout. The public `DeclareEvidence` input identifies the
criterion, exact source/artifact subject, producer/check/result IDs and hashes,
observation/expiry times, declared actor/reason, inert links and custom metadata.
There is no public passed/verified verdict. `evidence declare` appends a record;
`evidence supersede` appends an exact-hash correction with a reason, retaining the
original. Replays deduplicate by request/identity; equal-valued independent records
remain distinct. Supersession cannot fork or cycle.

`gate assess` requires explicit `ExactSubject` JSON and `--as-of`. The subject hash
is caller-declared until a supported evaluator admits it. Assessments compare current
captured gate/criterion definitions against evidence membership and freshness at
`as_of`; they do not reconstruct historical Git definitions. Missing, stale,
wrong-source, wrong-definition and unadmitted producer results remain explicit.
Declared evidence alone cannot yield a satisfied gate or qualify completion. Native
import/restore never upgrades provenance into verification.

Feature/gate mutations and immutable evidence records carry exact-document receipt
proofs validated before publication and on replay, events, staging and snapshot flows.
Prospective imports enforce authoring/reference/revision policy; exact historical
restoration preserves original declarations and paths. Exported schemas and actual
flags are discoverable through `schema` and `capabilities --json`.


## Agent context and continuity (PM-07)

`workdeck context --issue ID --budget 16384 --json --no-input` captures planning
state once and returns typed sections with citations and explicit omissions. The
CLI budget includes the compact JSON envelope and trailing newline; the shared
`ContextRequest.budget_bytes` bounds the packet itself. A budget below the
mandatory packet returns a structured error with the required packet size and
`minimum_stdout_bytes`. Projection applies after capture and cannot increase
that response budget; packet budget accounting describes the unprojected packet.
A diagnostic response has a separate 16 KiB ceiling, even when a requested context
budget is too small to encode a usable error.

The context anchor pins repository, selected issue source, requirements and a
fingerprint of the relevant captured inputs. Durable source identity uses relative
paths and content/availability information, so identical content in another clone
or after an identical atomic save retains the same anchor. Device/inode identity
still detects replacement during a capture, but does not enter the durable anchor.
It excludes packet formatting,
budget, assessment time and the handoff collection itself, so recording a handoff
does not immediately invalidate its own anchor. Requirements are distinct from
checked flags, comments, handoff summaries and imported completion declarations.
External instruction/source excerpts are bounded and confined to the captured
worktree; unsafe paths and nested repository boundaries are rejected or explicitly
omitted. Arbitrary explicitly selected planning roots have no assumed worktree.
Missing `--as-of` leaves evidence freshness unknown. A source citation or authored
evidence declaration does not establish a passing check.

Issue document links appear in a separate budgeted `documents` section as inert
references with `document_not_fetched`. Their citations pin the declaring issue,
not the linked document's contents. Paths and URLs are preserved without fetching
or interpreting them as executable navigation. Use explicit source links for
bounded worktree excerpts; omitted document entries remain visible in accounting.

`issue next` selects active unstarted work in priority, creation-time and ID order.
Blocking open questions, stale answered decisions and hard prerequisite conditions
exclude implementation selection. Empty selections return `selected: null`.
`next --issue ID` explains suggested actions with repository, issue and target
source tokens, graph, requirements and context preconditions. Suggestions grant
neither ownership nor execution authority. Candidate titles and conditions are
bounded and carry truncation/omission metadata. Pagination cursors bind the source
and query; changed inputs require restarting the read. Context exposes advisory
path and dependency overlaps without allocating worktrees or spawning agents.

Questions live under `.workdeck/questions/` with global `Q-<ULID>` identities.
They pin subject source tokens, optionally specific criteria, an actor and an
explicit `blocks_work` declaration. Create, answer and supersede use shared durable
operations. Answers never rewrite issue acceptance criteria. Subject edits make
old answers stale, and another answer cannot overwrite an answered question;
reconciliation uses explicit supersession. Open references block subject retirement;
answered and superseded records remain historical references. This coordination
policy is separate from completion gates.

Immutable issue-local handoffs use global `H-<ULID>` identities and a captured
context anchor. Their actor, summary, attempted work, uncertainties, evidence and
question references, pending operation identities and next steps are declarations.
An old handoff remains readable when its basis changes and is explicitly stale in
resumed context. Replaying the original request returns the original receipt even
after later source edits. Matching snapshot import cannot overwrite a handoff.

New agent-facing commands support compact JSON and bounded dotted field selection.
Selecting fields implies machine-readable output, including errors. Parser validation
failures for the new agent commands also use bounded machine errors when a machine
output flag is present, including misplaced machine flags before the command or on its namespace. Their source
state is `arguments_not_validated`; parsing does not inspect or initialize a source.
Help/version output retains the normal Clap behavior. Projection
retains envelope source identity; mutations also retain their complete receipt.
A post-write projection or staging failure explicitly records the committed
mutation, so the caller reconciles the original request instead of creating a new
one. Oversized diagnostic detail may omit a full receipt while retaining its lookup
identity and explicit partial-outcome flag. `retryable` permits inspection and
re-evaluation, never blind reuse of stale write preconditions.

The generated [command reference](reference/workdeck-pm-commands.md),
[schema catalog](reference/workdeck-pm-schemas.json) and
[PM skill](../skills/workdeck-pm/SKILL.md) come from the executable's typed catalogs.
Regenerate each with `workdeck protocol render commands|schemas|skill` and verify
parity with `cargo test -p workdeck-cli --test pm_catalog`. Rendering reads no
planning source and does not initialize a repository. `workdeck skill path workdeck-pm`
installs the current generated skill in the user's Workdeck configuration directory.
Repository instruction pointer installation is explicit and requires an initialized
native planning source. Preview the exact pointer and current `AGENTS.md` content
hash with `workdeck protocol preview --json`. Install using that
`--expected-content HASH`, or `--expect-absent` when the file was absent, together
with the preview's `--expected-repository` identity and a stable `--request-id`.
`protocol update` replaces an existing managed block.
Both operations preserve all bytes outside the block, reject malformed/duplicate
markers and unsafe files, and retain existing file permissions. Preview does not
recover interrupted writes or modify repository instructions.

These pointer writes use the shared PM writer lock but are local adapter operations,
not canonical planning mutations. Recovery journals and request receipts live under
ignored `.workdeck/.local/protocol/`. Reuse the original mode, request and source
precondition after an uncertain response; a completed request returns its original
receipt even if instructions have since changed. Conflicting changes require
reconciliation and never authorize replacing unrelated instructions. A local protocol
receipt grants no staging, commit, push or cross-clone authority. Losing local recovery
state also loses its retry history; this is not a distributed idempotency claim.

## PM-08 local commands and verification (integration in progress)

Repository authors place versioned command definitions in `.workdeck/commands/`,
checks in `.workdeck/checks/`, and flat profiles in `.workdeck/check-profiles/`.
Definitions retain authored YAML and accept extension data under `custom` or `x-*`.
A command declares typed argument tokens, an argv recipe or explicit shell recipe,
cwd, tools, environment, selected inputs, output bounds, artifacts and effects.
Effects are declarations; local recipes remain trusted executable code.

`command list|show|validate` and `check list|show|validate` inspect definitions.
`check profile list|show|validate` inspects flat check selections. `command plan ID`
and `check plan --check ID|--profile ID|--issue ID` capture an exact plan and save
its JSON under ignored `.workdeck/.local/plans/`. Discovery and planning do not
execute recipes. `--arguments-file` supplies typed JSON arguments.

Serialized plans are bounded to 4 MiB whether loaded by saved fingerprint, file
path or stdin. Argument/body input limits are separate. Oversized execution
compositions carry blockers; the runner also independently enforces at most 256
invocations and 64 MiB of declared retained logs/artifacts. Current planning is
stricter at 128 invocations. These are admission and retention bounds, not a disk
sandbox for arbitrary recipe writes.

Selected input capture defaults to at most 20,000 entries, 256 MiB per file and
1 GiB in aggregate. File hashing streams every selected byte, including bytes
beyond 64 MiB; exceeding either byte bound fails explicitly. This accommodates
large source catalogs and local tool binaries without omitting their content.
Planning-record lists reuse one validated retirement index within their snapshot,
so listing a large portfolio does not reparse operation history for every record.

Execute a reviewed plan explicitly with:

```sh
workdeck check run --plan PLAN_HASH --expected-plan PLAN_HASH \
  --actor agent-name --request-id stable-request-id --json --no-input
```

Use `command run` for a command plan. The expected plan must match its source,
requirements, definitions, selected input membership and bytes, tool identity,
and declared environment pins. Undeclared environment is cleared. Inherited
values are hashed in public manifests rather than disclosed. A plan may include
dirty source files; status rechecks relevant inputs and never converts a stale
historical pass into current success. Recursive selection explicitly excludes
Workdeck's own run/operation output and local engine state, not arbitrary generated
project directories. Unsupported or incomplete inputs block execution.

Canonical `runs/RUN-…/intent.yml` precedes spawn; `result.yml` records observed
completion. Each has its original durable operation receipt. Raw logs, artifacts,
process locks and recovery journals remain ignored under `.local/runs/`. Export
preserves canonical records and receipts, not local output files. Doctor and import
verify record/receipt agreement, and existing run records are immutable.

A repeated request returns or reconciles its original run without starting another
process. `check recover RUN-ID` reconciles retained state; an ambiguous interrupted
run is unknown. `check status`, `check results --status failed,stale,unknown`, and
`check explain` expose current assessment separately from historical observations.
Result pagination binds its cursor to source and result membership.

Process exit, report assessment, source freshness and artifact availability are
separate dimensions. JUnit support is a bounded UTF-8 XML 1.0 testsuite/testcase
contract with exact expected suite names and minimum executed tests. Empty,
all-skipped, missing or inconsistent reports cannot establish passing checks.
SARIF support is a bounded 2.1.0 subset requiring a matching tool, explicit completed
invocations and results; unresolved external property files cannot establish a
clean analysis. Neither adapter fetches external resources.

A passed execution exits zero; failed exits 1, stale/running 4, blocked/not-run/skipped
5, unknown 6, and canceled 130. Read-only status inspection can succeed while
reporting a nonpassing run. JSON run output preserves run/request identity and
full mutation receipts even when fields are projected. A post-commit rendering or
staging error retains recovery information rather than implying nothing happened.

Context and the terminal Checks section show recent bounded summaries and source
references. Execution is explicit; the TUI owns one foreground worker, waits for
cleanup on cancellation/exit, and defers suspension while it is active. These
results are local feedback. CI trust and required-check completion admission are
separate PM-11 work and remain unavailable at this checkpoint.


### Indexed reviewer and deadline predicates

Disposable projection format 2 adds reviewer and normalized deadline predicates.
Old projections require rebuild by an explicit refresh; cache-only reads do not
silently reinterpret older data. This does not change authoritative document schema 1.
`IssueQuery` accepts optional `reviewer`, `workflow_categories`, and `due_before`.
Unused new predicates are omitted from serialized queries, preserving existing
saved-query and mutation-intent representations.

`repository my-work --facet review-requested` selects the actor as reviewer in the
configured Review workflow category. `--facet overdue --as-of RFC3339` selects
nonterminal assignments before that exact observation time. Date-only deadlines
become overdue after their UTC calendar day; timestamp comparison preserves offsets
and nanoseconds. Facet, actor, evaluation time and source identities bind pagination.
The original `--assignee` flag names the actor for all facets and stays compatible.


## Imported signed-report authority

`AttestationId` uses `ATST-<canonical ULID>`. The immutable
`.workdeck/attestations/<ATST-ID>.json` record contains repository and request identity,
import time, exact input envelope text, policy snapshot, externally supplied policy
and commit pins, importer attribution, and the derived historical admission summary.
Its `evidence.import_report` receipt binds exact record bytes and request inputs.
The snapshot file kind is `attestation`; records cannot be overwritten by native merge.
Doctor detects missing or edited records and missing/contradictory import receipts.

`ImportedCheckReport` is historical provenance. Current authentication requires the
original signed bytes plus an independently selected policy fingerprint and expected
commit. Embedded policy snapshots and serialized authentication results are not
current trust roots. `EVD` declarations remain separate; importing an attestation does
not grant criterion or completion acceptance. The generated schemas expose
`attestation-id`, `import-check-report-request`, `imported-check-report` and
`imported-check-report-record` and `imported-check-report-summary`.
The summary omits the original input and document payload. Activity projections use
import time, importer, producer and historical observed status. Disposable projection
schema 3 rebuilds older caches to populate these fields.


## CI baseline pin

`ci-baseline-pin` contains an exact `commit` and evaluation `contract` fingerprint.
Pinned validation returns the supplied `baseline_pin` and basis
`pinned_baseline_validation` only after recapturing and comparing the baseline.
Ordinary validation omits `baseline_pin` and retains `planning_source_validation`.
A pin is an external caller input, not a signed reviewer credential. Required contract
review and candidate validity are evaluated unchanged.


## Authenticated contract review

`contract-review-policy` binds a repository and 1–16 required reviewers. Each reviewer
has a unique ID and unique nonweak Ed25519 public key, with a bounded validity interval.
The entire policy is independently pinned. All reviewers must sign the same
`ci-contract-approval` payload; duplicate signatures or key aliases cannot substitute
for another reviewer.

`signed-contract-review` uses a distinct DSSE payload type, exact signed JSON bytes,
and bounded signatures. Policy input is at most 64 KiB, envelope input 256 KiB and
decoded payload 64 KiB. Approval binds schema, repository, baseline commit/contract,
full candidate `CiSourceIdentity`, candidate contract hash, decision, reviewed time
and expiration. Both current reviewer validity and validity at review time are checked.

`ci-reviewed-validation` retains the ordinary validation report alongside
`authenticated-contract-review` admission. A changed contract can pass the combined
review gate while the retained ordinary report continues to state that review was
required. Structural/organization errors cannot be approved away. This authority is
for evaluation-contract review, separate from check producers and completion evidence.


## Retained contract reviews

`ContractReviewId` uses `CRVW-<canonical ULID>`. The
`.workdeck/contract-reviews/<CRVW-ID>.json` record stores the original UTF-8 DSSE
envelope, policy snapshot, baseline and candidate pins, importer, import time and
historical authenticated admission. `review.import_contract` receipts bind exact
input, record identity and create-only bytes. The native snapshot kind is
`contract_review`; merge cannot overwrite a differing existing record.

The generated contracts expose `contract-review-id`, `import-contract-review-request`,
`imported-contract-review`, `imported-contract-review-record` and
`imported-contract-review-summary`. The summary omits envelope/policy/document
payloads. Records are bounded at 1 MiB; the catalog is bounded at 256 records/16 MiB.

Historical validation checks retained signature proof at the original import time
and verifies its original receipt. It does not recertify current authority. Current
reauthentication recaptures immutable Git sources and requires externally supplied
baseline/candidate/policy pins. Historical Activity metadata includes importer,
import time, reviewer names and approved decision; opaque signed payloads are not
indexed as search text. These records do not grant criterion/completion acceptance.


## Review coverage and context

`review-coverage-request` selects a `CiRevision`, optional `CiSubjectIdentity` and
exact selected document hash. `working_tree` defaults to false; when true it also
compares live planning contracts and the committed evaluator selection. Optional
authority contains independent baseline pins,
reviewer policy and its expected fingerprint. `review-coverage` reports the captured
source/subject, diagnostics and bounded retained-review rows. States distinguish
`historical_match`, `authenticated`, `stale`, `rejected` and `unknown`. Only freshly
verified external authority can produce `authenticated`; current reviewer IDs remain
separate from each row's historical admission. The report fingerprint excludes the
observation timestamp, while retaining state, proof membership and source/policy pins.
Optional report `working_tree` contains the observed `contract` and `evaluators` hashes
and `matches_revision`. An unavailable capture adds diagnostics and cannot authenticate.
Evaluator comparison uses descriptor-safe bounded reads, exact bytes, executable modes
and full selected tree membership, including untracked files. The candidate commit
selects evaluators; a dirty recipe cannot shrink that comparison. Live planning uses
exact contract document pins, so cosmetic document edits may also invalidate coverage.

Context adds a `reviews` section containing `contract_review` content, selected HEAD
and exact planning-source citations. At most five proof rows and two diagnostic notices
are displayed, with total/omitted counts. The complete assessment fingerprint binds
the context anchor and is rechecked before return; it does not alter the authored
requirement fingerprint. Repositories without retained review history retain their
previous context anchor encoding and omit the new section. Context observations use
no current reviewer authority and cannot grant completion. They enable working-tree
comparison automatically and recheck evaluator observations before returning. Changes
between two different stale evaluator states also change the context fingerprint;
application files outside declared evaluator selection are not certified.

The TUI's explicit current-review assessment is separate from `ContextPacket`.
Its per-task form holds independently supplied `ReviewCoverageAuthority` in memory
and invokes `Repository::contract_review_coverage` with `working_tree: true`, HEAD
and the inspected issue document hash. It does not replace packet anchors or imply
that historical packet rows were authenticated. Input edits, capture errors and
discard invalidate the displayed current assessment; refresh reuses only explicitly
submitted authority. No persisted schema or mutation operation is added.

## Red/green check contracts and assessment

`CheckDefinition.red_green` optionally holds `red-green-requirement`: unique accepted
`red_exit_codes` (1–16 values in 0–255) and 1–128 structured `cases`. Each case has
`suites` (full ancestor path), `class_name` and `name`, and belongs to a required JUnit
suite. Process-only and SARIF declarations cannot request this JUnit assertion gate.
Definitions omitting the new field retain their previous encoding. The declaration
is part of the independently captured check contract and producer definition scope.

`junit-case-identity` and `junit-case-report` expose inert case inventories with separate
passed/failed/error/skipped outcomes. The bounded parser sorts by structured identity
and rejects duplicate full identities across repeated suite paths. `::` inside class
or case names does not alias another structured identity. Passing case inventory is
not appended to legacy `ReportAssessment` or retained run documents; their format
remains unchanged. Red/green verification instead requires the original XML bytes
and compares their hashes with authenticated artifact descriptors.

`red-green-request` contains independent red baseline pins, exact candidate commit,
check ID, producer policy/hash, original signed reports and UTF-8 XML artifacts.
`red-green-assessment` identifies the authenticated check pair, sources, producers,
run IDs, original payload/artifact hashes, required cases and changed selected inputs.
Its fingerprint excludes observation time. Consumers must reverify original proofs;
this value grants neither baseline acceptance nor criterion/completion authority.

Projection schema 4 invalidates older disposable indexes so unchanged check files
are revalidated under the typed red/green declaration rules. Authoritative planning
files and report/receipt formats are unchanged by this cache version bump.


### Reviewed red baseline admission

`RedGreenBaselineReview` supplies an independently accepted prior commit/contract,
current reviewer policy and fingerprint, and original signed contract-review envelope.
`verify_reviewed_red_green` calls the shared reviewed validator for prior baseline to
red commit, requires the exact proposed red contract and ancestry, then verifies the
unchanged red-to-green contract and original signed check artifacts. The combined
`ReviewedRedGreenAssessment` retains separate reviewer admission and producer pair
observations. Neither serialized observation replaces original proofs on later use.
This operation does not update accepted refs or store completion evidence; those
mutations use separate explicit authority and remain required integration work.


### Retained red/green proof

`ImportCheckReportRequest.red_green` optionally embeds a `RetainedRedGreenProof`
with the accepted red commit/contract, check ID, original signed red report, exact
UTF-8 red/green XML and optional original signed baseline-review proof. The green
envelope and producer policy remain in the enclosing import. Red/review envelopes
retain their exact signed payload and signature fields as typed DSSE objects; their
outer JSON whitespace is not an identity. The green envelope retains its original
text under the existing import contract. Omitted fields retain
legacy request/receipt encoding. The complete ATST JSON record remains bounded to
8 MiB; each artifact separately remains bounded to 4 MiB. Aggregate import/catalog
and transaction limits also apply.

Import verifies the live immutable pair before the existing transaction publishes
the record. Offline reads, recovery and snapshots validate original signed report
and artifact consistency at historical import time. That history makes no current
Git/source, reviewer or completion claim. `RetainedRedGreenAuthority` supplies fresh
external red baseline, candidate, producer policy/hash and optional current reviewer
policy/baseline/hash. Reauthentication uses retained original signatures and XML,
recaptures Git inputs and rechecks the accepted contract. A reviewed stored proof
cannot discard reviewer authority, and a missing review cannot be synthesized.
`RetainedRedGreenAssessment` keeps reviewer admission separate from the check pair.
The normal green-report reauthentication API remains producer authentication only.


### Exact criterion-to-attestation links

An `EvidenceLink` can use `kind: attestation` with an `AttestationId` and exact record
`content` hash. At most one attestation can be selected per declaration; changing it
uses immutable supersession. Authoring this link remains declared provenance and
may retain historical references. It does not assert execution or criterion acceptance.

`RedGreenEvidenceRequest` requires an evidence ID/content pin and independently
obtained `RetainedRedGreenAuthority`. Shared verification requires an active,
unexpired declaration; the original receipted attestation; matching current and
committed criterion definitions; exact candidate `content`, check/producer refs,
green run ID, signed report fingerprint and signed observation time. The current
criterion may carry a checked/unchecked declaration, which is reported separately;
that declaration does not become an execution verdict. Retired owners fail.

The verifier recaptures immutable Git criterion source and rereads current evidence,
attestation and criterion after proof verification. Changes invalidate the result.
`RedGreenEvidenceAssessment.basis` is `authenticated_check_link`; current and committed
criterion source pins and original attestation identity remain explicit. Gate-policy
mapping and completion acceptance are separate, still-required evaluation steps.

`CriterionOwner::Project` resolves project `exit_criteria`; existing milestone owners
continue resolving `outcomes` with unchanged identity. Project subjects participate
in question validation, retirement blockers, issue context and projection citations.

Projection schema 5 rebuilds disposable caches to include nested question subjects,
direct question criterion references, and evidence declaration criterion/attestation
citations. These edges retain the citing document path/hash and do not authenticate
the referenced attestation or grant completion authority.


### Authenticated committed gate requirements

`red-green-gate-request` supplies the exact gate ID/source token, one independent
`retained-red-green-authority`, and one `GateEvidenceSelection` per AND requirement.
Selections contain requirement ID, evidence ID and expected evidence content hash;
serialized assessments are never inputs. Missing, duplicate and unknown requirement
IDs fail. Every selection must freshly verify its original attestation, match the
gate's criterion/check/producer, and qualify the same candidate. Explicitly unchecked
criteria remain blocked. The current gate must match its exact committed source and
definition; current or committed archival/retirement cannot qualify.

The verifier captures the gate, criteria, configuration, evidence catalog and original
attestation pins before verification and compares them again afterward. It checks
current evidence activity, observation time, expiry and requirement maximum age, plus
producer/reviewer/review expiry before returning. Reused evidence is verified once
within the call, with conflicting content pins rejected. No cross-call verdict cache
or source mutation is introduced. `red-green-gate-assessment` returns the committed
candidate, gate and per-requirement original-proof assessments with basis
`authenticated_requirements`. This is a current gate-policy assessment; issue Done,
project/milestone exits and feature maturity still require their shared mutation-policy
integration. Ordinary `gate assess` retains its declaration-only behavior.

### Source-bound red/green issue completion

`complete-red-green-issue` is a request: issue/source pin, actor, independent
`retained-red-green-authority`, exact check-to-attestation selections, and explicit
gate/evidence selections. `red-green-issue-completion` is the historical publication:
original request, before/after issue records and document bytes, admitted completion
report, current plan/committed binding, original attestation/evidence records, gate
assessments and admission time. These result objects are never accepted as live
completion authority.

The private admission type cannot be deserialized. Live preflight prepares all
configured required checks and profiles for the issue, binds planning and input
manifests to the selected candidate, requires that candidate to be HEAD, verifies
original proof, and checks gate selections. Under the transaction lock, completion
revalidates the plan, original record pins, active evidence and criteria, and runs
ordinary description/criteria/dependency/workflow/organization policy. Just before
journaling, it verifies HEAD, captured execution inputs and authority freshness again.
The `authenticated_checks` basis describes required-check qualification; ungated
checkbox criteria still retain their declared/manual semantics.

The new operation is `issue.complete_red_green`; it publishes the same issue state
transition through the shared mutation implementation and retains its proof in the
operation receipt. It preserves request replay before external verification, interrupted
journal recovery, and before/after source identities. Typed proof validation runs
before publication and in history, recovery, snapshots, doctor inspection and staging.
Historical parsing uses the recorded admission time and checks retained original
signed evidence; it does not perform fresh Git qualification or renew trust.

This first completion path requires red/green proof for every selected required
check. The current green-only check and attached-gate paths are separate typed
operations; claimed completion and project/milestone/feature policy transitions
remain unfinished requirements.


Projection schema 6 invalidates prior caches so completion receipts previously
accepted as generic operations receive the new typed original-proof validation.
The live completion admission also retains its original Git directory/worktree
binding through publication; identical commit content in a replacement Git directory
does not authorize completing work under the old preflight.

### Current imported-check qualification

`VerifyImportedCheck` (`verify-imported-check` schema) supplies the exact attestation
ID/content, check ID, candidate commit, independent producer policy/fingerprint and
optional current red/green authority. `Repository::verify_imported_check` returns an
`AuthenticatedCheckReport` only after the selected check and invocation pass and the
original plan matches current committed definitions and invocation inputs. The
candidate must be current HEAD. Configured red/green requirements and retained pair
proof require matching current authority; green-only reports cannot replace them.
The original Git binding and captured inputs are checked before returning. No
operation is written; this returned report is not a deserializable mutation admission.
The existing authentication-only API continues to authenticate both passed and
failed signed reports, keeping attribution separate from successful qualification.


### Authenticated green-only gate evidence

`VerifiedEvidenceRequest` pins an active evidence declaration, its exact immutable
attestation record and a `CompletionAuthority`. The verifier requires an exact source
subject, matching criterion/check/producer/result/observation fields and a signed
passed check with unchanged invocation inputs. It recaptures the committed criterion
and evidence/attestation bytes before returning `VerifiedEvidenceAssessment` with
basis `authenticated_check`. Configured or retained red/green records still require
their pair authority; a green-only request cannot weaken that policy.

`VerifiedGateRequest` selects one such declaration for every committed AND requirement.
`VerifiedGateAssessment` binds the exact current gate, candidate source and per-
requirement authenticated check evidence. The read-only `gate verify-green` command
uses these contracts and rejects missing/duplicate requirements, stale or over-age
evidence, changed gates/criteria/source, and red/green requirements without their
original pair proof. New verified completion receipts can retain either this
green-only assessment or the legacy `RedGreenGateAssessment`; historical validation
rechecks the retained bytes and signed report at admission time.


### Transactional green-only issue completion

`CompletionAuthority` supplies an independently obtained candidate, producer policy
and policy fingerprint, with optional retained red/green authority. `CompleteVerifiedIssue`
uses that authority with one exact imported attestation selection for every required
check and the issue's complete attached-gate selection. Live admission is private and
is constructed only after current HEAD, committed check definitions, invocation inputs,
issue source and organization/workflow policy are revalidated. A selected check must
have a signed passed result and a passed, unchanged invocation; authentication of a
failed or incomplete result never qualifies completion. Configured red/green checks,
or imported records retaining a red/green pair, still require the original pair and
fresh matching authority.

`VerifiedIssueCompletion` is the historical receipt proof for `issue.complete_verified`.
It retains the exact request, before/after issue documents, prepared plan, imported
records, gate/evidence proof and admission time. The transaction holds the original
Git directory binding through final publication, checks source and evidence again at
the journal boundary, and preserves exact request replay and interrupted recovery.
Receipt validation runs in history, recovery, snapshots, doctor and staging. The
green-only operation proves authenticated required checks and can retain the typed
green-only attached-gate assessment; claimed completion and project/milestone/
feature maturity transitions remain separate policy integrations.


### Claimed authenticated completion

`CompleteClaimedVerifiedIssue` combines a current `CompleteClaimedIssue` ownership
claim with a separately obtained `CompleteVerifiedIssue` proof. The claim remains the
authority for who may complete the issue; the verification document supplies the
authenticated candidate, producer policy, attestation selections and attached-gate
proof. The operation rejects mismatched issue, actor or source tokens and does not
accept release flags through the CLI variant.

The transaction revalidates the claim and verified completion admission together,
holds the source and captured execution-input guards through the journal boundary,
and stores `ClaimedCompletionVerification` inside the existing claimed receipt. Exact
request replay returns that receipt, including when Git is unavailable later. History,
recovery, snapshots and doctor validation recheck the retained synthetic verified
proof at the recorded admission time; they do not renew claim ownership or external
producer authority. The older local/shared claimed-completion and separate-release
contracts remain compatible, while broader project/milestone/feature policy
transitions are still separate work.

The native claims workbench exposes the same contract through `g` and a bounded
verification-file form. It retains the parsed proof and path across a lost response,
uses the original request ID on retry, and renders the receipt as authenticated
completion without implying a release.


### Shared hierarchy and feature maturity policy

`PolicyAcceptance` is the explicit attributed acceptance used when a planning
record or feature is promoted. It contains a bounded `actor` and `reason`; the
actor is checked against the repository organization policy. Acceptance is kept
separate from CI, reviewed evidence, and local feedback.

`PolicyAssessment` is a read-only, source-bound result. It identifies the
project, milestone, or feature subject; current and requested state; policy basis;
allowed status; every condition; all source pins; a deterministic fingerprint;
and any supplied acceptance. Conditions expose issue/member status, declared
criteria, project-owned milestones, feature prerequisites, retirement state, and
feature gates independently. Unknown conditions remain blocking and are not
treated as declarations of success.

The native commands are:

```sh
workdeck project assess PROJECT_ID --json
workdeck project complete PROJECT_ID --actor ACTOR --reason REASON \
  --expected-revision REVISION --expected-content CONTENT --json
workdeck milestone assess MILESTONE_ID --json
workdeck milestone complete MILESTONE_ID --actor ACTOR --reason REASON \
  --expected-revision REVISION --expected-content CONTENT --json
workdeck feature assess FEATURE_ID --to specified --json
workdeck feature promote FEATURE_ID --to specified --actor ACTOR --reason REASON \
  --expected-revision REVISION --expected-content CONTENT --json
```

Project and milestone completion requires at least one completed issue member
and explicit declared criteria accepted by the supplied actor. Feature maturity
advances one stage at a time; `specified` requires an accepted decision and a
criterion, while `implemented` additionally requires completed associated issues
and mature prerequisites. A feature with declared gates remains unknown until
the authenticated gate path is integrated. Every mutation re-evaluates the live
snapshot under the transaction lock and preserves the normal immutable receipt.
These policy operations do not create a reusable completion credential. The TUI
exposes `p` for Projects/Milestones and `m` for Features with the same source
preconditions; authenticated policy-basis integration remains in progress.
