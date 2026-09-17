# Workdeck planning collaboration

This describes the implemented PM-09 command and workbench flow. The
[implementation ledger](project-management-implementation-plan.md#2110-pm-09--add-explicit-git-collaboration-and-claims)
tracks remaining integrated qualification. Indexing and CI qualification belong to
later phases; a claim or local check result does not supply CI acceptance.

## Configure planning authority

Keep planning records in the repository-root `.workdeck/`. Shared mode adds this
fragment to the existing `.workdeck/config.yml`, preserving its repository identity
and other configuration:

```yaml
sources:
  remote: origin
  accepted_ref: refs/heads/main
  coordination_ref: refs/heads/workdeck-coordination
  proposal_namespace: refs/heads/workdeck-proposals
```

The accepted ref holds accepted planning requirements. Proposal refs contain
reviewable planning changes. The isolated coordination ref holds claim history;
it does not become an application branch. Configuration requires distinct,
non-overlapping full ref names. Local mode omits `sources` and coordinates only
clients using that planning source.

Shared Git operations use an isolated configuration profile: global/system Git
configuration is disabled, includes are rejected, and the selected remote must
have matching fetch and push destinations. Current owned-process guarantees use
Unix process facilities. Inspect structured failures rather than assuming another
configuration or platform provides the same guarantees.

## Inspect, then execute

```sh
workdeck capabilities --json
workdeck source status --json
workdeck claim contract "$ISSUE" --json
```

A source status contains the configuration fingerprint and, when available, the
Git/remote binding fingerprint. Local mode or an unavailable publication binding
can return no binding. A work contract contains the issue source token and accepted
requirements. Keep these separate: requirements are portable across clones; the
inspected publication binding identifies this local Git directory and destination.

In the examples below, shell variables represent values copied from those JSON
responses. Each new logical mutation needs its own valid request ID. Preserve that
ID and all its original arguments for retries; do not substitute newly observed
preconditions into an old request.

Fetch and sync are explicit:

```sh
workdeck source fetch --expected-config "$CONFIG_HASH" \
  --expected-binding "$BINDING_HASH" --request-id "$FETCH_REQUEST" --json
workdeck source sync --expected-config "$CONFIG_HASH" \
  --expected-binding "$BINDING_HASH" --request-id "$SYNC_REQUEST" --json
```

Fetch refreshes cached source observations. Sync also materializes ignored local
planning views. Both preserve the developer's branch, index, and code worktree.
Ordinary source inspection does not fetch. Bound Git commands explicitly use
`--no-lazy-fetch`, so a missing object in a partial clone produces a read failure instead
of implicitly contacting its promisor remote. Use the explicit reviewed fetch command to
obtain missing objects. Git must support this global flag; unsupported versions fail
closed. The local qualification used Git 2.50.1 (Apple Git-155). Locally available shared refs remain
readable when their publication binding is unavailable, with explicit unknown
freshness; that read does not authorize publication.

## Claim work and retain the result

```sh
workdeck claim acquire --contract "$CONTRACT_HASH" \
  --expected-contract "$CONTRACT_HASH" --actor "$ACTOR" \
  --expected-binding "$BINDING_HASH" --request-id "$CLAIM_REQUEST" --json
workdeck claim status "$ISSUE" --json
```

Local mode omits `--expected-binding`. Shared CLI mutations require the inspected
binding. A changed destination or replaced Git directory rejects the original
reviewed action. Refreshing source information does not change a retained request.

Save the returned token, generation and content precondition. Renew, release,
cancel and supersede use that exact precondition and actor. Revalidation additionally
requires an explicitly inspected replacement work contract. Recovery requires the
expired claim's precondition and an explanation; expiry alone grants no authority
to take over or keep working.

Cached claim reads have an unconfirmed shared guarantee. Publication outcomes
separate the original receipt from the current claim assessment. Use the outcome's
`may_continue`: it requires both `requested_token_current` and a usable current
assessment. An old acquisition receipt does not authorize its holder to use a
later owner's valid claim. Shared confirmation describes the remote state at
observation time; each subsequent mutation checks its own ownership preconditions.
If the post-publication observation budget expires, any existing receipt and
confirmation remain available while continuation is denied. Claims do not stop or
fence an external agent process.

Every newly admitted PM transaction, including writes unrelated to claims, checks
that its exact durable receipt fits within 10,000 receipt files and 64 MiB of encoded
operation history. Exceeding either limit rejects the new write before application
files change. Existing receipts are retained; the new-admission limit does not
invalidate historical replay or an already journaled operation's recovery.

## Complete, then release separately

```sh
workdeck claim complete "$ISSUE" --actor "$ACTOR" \
  --token "$TOKEN" --generation "$GENERATION" --expected-content "$CLAIM_CONTENT" \
  --expected-issue-revision "$ISSUE_REVISION" --expected-issue-content "$ISSUE_CONTENT" \
  --contract "$CONTRACT_HASH" --expected-contract "$CONTRACT_HASH" \
  --expected-binding "$BINDING_HASH" --request-id "$COMPLETE_REQUEST" \
  --release-request-id "$RELEASE_REQUEST" --release-reason "Work completed" --json
```

Completion requires the current actor, exact claim precondition, unexpired ownership
and strict completion policy. In shared mode, the local issue source, requirements
and exact PM configuration must agree with confirmed accepted authority. Proposed
requirement changes cannot substitute for that accepted contract. Completion writes
the local issue; it does not promote that issue to the accepted ref. The optional
release is a separate operation with a distinct request ID. Inspect `completion`,
`release_recorded`, `release_error`, and `release_publication` independently.
Uncertain release publication does not undo the completed issue or count as a
recorded release. Retry the original composite request to reconcile its history.
A destination change between completion and release preserves completion and
rejects the redirected release. `release_recorded` describes the original requested
release, not whether a later generation now owns the issue.

## Publish a reviewed planning proposal

```sh
workdeck source proposal preview --ref "$PROPOSAL_REF" --title "$TITLE" --json
workdeck source proposal publish --plan "$PLAN_HASH" \
  --expected-plan "$PLAN_HASH" --request-id "$PROPOSAL_REQUEST" --json
workdeck source proposal status --request-id "$PROPOSAL_REQUEST" --json
workdeck source proposal resume --request-id "$PROPOSAL_REQUEST" --json
```

Preview retains the reviewed changes, exact source/configuration/binding, accepted
application base, and expected existing proposal. Publication uses that plan and
preserves application content from the accepted base. It never directly pushes the
accepted ref. Reviewing and merging a proposal remains a separate Git operation,
including when the accepted branch is protected.

Status inspects retained local publication history. Resume explicitly reconciles
the original request. An uncertain push is not retried as a newly prepared claim or
proposal merely because the observed remote tip is unchanged.

## Use the terminal workbench

From Issues, press `i` for Context, then `6` for Sources or `7` for Claims.

- Sources preserves immutable opened citations. `r` captures a new observation;
  Enter explicitly adopts it as the opened document.
- In Sources, `o` opens operations. `f`/`s` inspect Fetch/Sync; `x` executes the
  reviewed action. `n` opens a proposal form; Ctrl-S previews and `x` publishes.
  `v` inspects proposal status, `u` resumes, and `t` opens an original request ID.
- Ctrl-D discards the retained local source-operation intent explicitly. It does
  not undo a published operation.
- Claims provides acquisition, renewal, revalidation, release and recovery forms;
  `e` completes and separately releases. Retained forms keep original identities
  and preconditions through errors and Review/Issues switching.
- F2 opens Review and F3 returns to planning. Quit, suspend and interrupt requests
  defer while an owned publication worker finishes and is joined; they do not
  manufacture a canceled publication result.

Use `workdeck doctor --staged` to validate the actual staged planning snapshot and
its relation closure. Hook installation is explicit: inspect `workdeck hooks
preview --help` and apply the reviewed hook plan with its exact fingerprint.
