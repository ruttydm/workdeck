# Agent Context Service Integration

Date: 2026-08-25
Status: Proposed integration boundary; not implemented
Last verified: 2026-08-25
Canonical product research: private product-research archive

## Decision

Workdeck should consume an agent context service, not absorb one.

The context service is a local-first daemon that preserves cross-agent session evidence, derives governed memory, retrieves relevant history, and compiles cited context packs. Workdeck remains the durable review workspace for current repositories, worktrees, checkpoints, semantic units, artifacts, and human decisions.

This boundary lets both products remain coherent:

- the context service works from any editor, terminal, agent, or MCP client;
- Workdeck can enrich review without owning every agent's storage format;
- raw transcripts and derived memories do not enter the Workdeck ledger;
- Workdeck's immutable checkpoints can provide precise retrieval scope;
- either system can evolve, fail, be removed, or be replaced independently.

## Responsibility Boundary

| Concern | Context service | Workdeck |
|---|---|---|
| Agent-session discovery and import | Owns | Does not own |
| Immutable raw evidence | Owns | Stores only references needed by a frozen attachment |
| Canonical session/event model | Owns | Does not own |
| Derived decisions, gotchas, attempts, procedures, and open threads | Owns provenance and lifecycle | Presents relevant claims and captures user feedback |
| Lexical, semantic, temporal, file, symbol, and Git-aware retrieval | Owns | Supplies the current review scope |
| Context-pack compilation | Owns | Requests, previews, freezes, and attaches packs |
| Projects, repositories, and worktrees | Resolves identities for retrieval | Owns catalog and local working-set UX |
| Decks and immutable checkpoints | References | Owns |
| Semantic review units and review decisions | May use stable references | Owns |
| Pull requests, CI runs, and review artifacts | May cite imported evidence | Owns the review presentation and ledger |
| Data retention, redaction, and forgetting | Owns session evidence and memory | Owns Workdeck attachments and review records |

Neither side writes directly to the other's database. The integration uses a versioned local API, CLI JSON, or MCP contract.

## Combined User Experience

### 1. Explain a changed unit

The reviewer selects a semantic unit in a checkpoint and opens **Prior context** in the inspector. Workdeck sends repository identity, base/head commits, path, symbol identity, semantic-unit kind, and a bounded excerpt or content hash.

The service returns:

- prior sessions that touched the same code or closely related symbols;
- accepted decisions that still appear current;
- failed approaches and their observed outcomes;
- open threads related to the unit;
- contradictions, staleness, and confidence warnings;
- resolvable evidence citations.

Workdeck shows the compact claims first. Expanding a citation opens exact evidence in a read-only view and makes clear that it comes from the external context store.

### 2. Prepare an agent handoff

From a deck or checkpoint, the reviewer chooses **Build context pack**, enters or selects the next task, and sets a token budget. Workdeck adds the selected review scope and current Git state; the service compiles the historical context.

The reviewer can:

- inspect why each item was included;
- remove irrelevant or sensitive items;
- request a smaller or larger budget;
- accept, reject, or supersede questionable memories;
- copy Markdown, save JSON, or pass the pack to an MCP-capable agent;
- freeze the exact pack as a content-addressed Workdeck attachment.

A frozen attachment is reproducible review evidence. It does not become a second mutable memory database.

### 3. Resume interrupted work

When a deck source advances after an agent run, Workdeck can request sessions correlated with the worktree, commit range, files, and time window. This helps the reviewer find the run that produced the change even if the originating agent did not create a formal handoff.

### 4. Catch contradictions

If retrieved memory conflicts with the current source, Git state, or another accepted claim, Workdeck displays the conflict instead of selecting a winner. A reviewer can mark the old claim superseded through the service and attach the new evidence.

## Architecture

```text
Codex / Claude / Cursor / Zed / terminal agents
                       │
                       ▼
       ┌────────────────────────────────┐
       │ Local agent context service    │
       │                                │
       │ evidence → memory → retrieval  │
       │              → context packs   │
       └───────────────┬────────────────┘
                       │ versioned local contract
                       │ read + explicit feedback only
                       ▼
       ┌────────────────────────────────┐
       │ Workdeck                       │
       │                                │
       │ catalog → deck → checkpoint    │
       │         → semantic unit/review │
       └────────────────────────────────┘
```

Workdeck should treat the service like another optional provider, similar in spirit to GitHub or local Git integration but with stricter privacy and provenance requirements.

## Identity Contract

Path-only matching is unsafe because Workdeck supports multiple repositories and linked worktrees. Every request should include as much of the following identity as is known:

```json
{
  "repository": {
    "workdeck_repo_id": "repo_...",
    "remote_urls": ["git@github.com:owner/repository.git"],
    "root_commit": "optional-root-or-lineage-id"
  },
  "worktree": {
    "workdeck_worktree_id": "worktree_...",
    "path": "/local/path",
    "branch": "feature/context",
    "head": "0123456789abcdef"
  }
}
```

The service returns its resolved repository and worktree IDs plus match confidence. Workdeck warns on ambiguous or low-confidence identity rather than attaching context silently.

Forks, remotes changed after cloning, copied directories, rebases, and detached HEADs need explicit fixtures. The integration must never equate repositories solely because their directory names match.

## Read Contract

The implementation may use MCP, a local HTTP/Unix-socket API, or a CLI adapter. Workdeck should depend on semantic operations rather than transport-specific response shapes.

Minimum operations:

```text
get_status()
resolve_scope(repository, worktree, revision)
search_history(scope, query, filters, limit)
get_evidence(evidence_id, range?)
build_context_pack(scope, task, budget, policy)
explain_change(scope, base, head, unit?)
get_open_threads(scope, filters?)
record_feedback(target_id, action, reason?)
```

All read results include:

- service and schema version;
- stable result or pack identifier;
- scope resolution and confidence;
- source citations;
- derivation metadata for non-primary claims;
- freshness or invalidation warnings;
- truncation and token accounting;
- policy-based omissions;
- a human-readable inclusion explanation.

## Suggested Data Shapes

### Evidence reference

```json
{
  "evidence_id": "ev_...",
  "source": "codex",
  "session_id": "session_...",
  "event_id": "event_...",
  "occurred_at": "2026-08-25T09:42:00Z",
  "locator": {
    "kind": "message_range",
    "start": 1842,
    "end": 2198
  },
  "content_hash": "sha256:...",
  "redaction_state": "none"
}
```

### Memory claim

```json
{
  "memory_id": "mem_...",
  "kind": "decision",
  "summary": "Review state attaches to semantic unit versions.",
  "status": "accepted",
  "confidence": 0.96,
  "scope": {
    "repository_id": "repo_...",
    "paths": ["crates/workdeck-core"]
  },
  "evidence": ["ev_..."],
  "supersedes": [],
  "derived_at": "2026-08-25T10:00:00Z",
  "deriver": "rules-or-model-version"
}
```

### Context pack

```json
{
  "pack_id": "pack_...",
  "purpose": "review semantic unit unit_version_...",
  "scope": {
    "repository_id": "repo_...",
    "base": "abc123",
    "head": "def456",
    "unit_version_id": "unit_version_..."
  },
  "budget": {
    "requested_tokens": 6000,
    "estimated_tokens": 5740
  },
  "sections": [],
  "citations": [],
  "warnings": [],
  "compiler_version": "context-pack/v1"
}
```

IDs from the external service must be namespaced in Workdeck storage so they cannot collide with Workdeck entity IDs.

## Workdeck Storage

Workdeck stores only what is necessary for review continuity:

- provider configuration and health, without secrets in repository config;
- external IDs and resolved scope metadata;
- a cache with explicit expiry, safe to delete;
- user-visible feedback pending delivery when the service is offline;
- content-addressed context-pack attachments explicitly frozen by the user;
- the pack's citations, compiler version, policy, and integrity hash;
- review events that record when a pack was attached or consulted.

Mutable search results and derived claims should not be copied into the review ledger by default. If the user freezes a pack, Workdeck preserves the exact rendered Markdown and structured manifest so a future review can reconstruct what was known at that checkpoint.

## UI Surfaces

### Inspector: Prior context

Add an optional inspector section for the selected repository, checkpoint, file, or semantic unit:

```text
PRIOR CONTEXT                                    [Refresh]

Decision · accepted · 3 citations
Review state attaches to semantic unit versions.
Why included: same symbol + same repository

Failed attempt · confidence 0.91
Path-only worktree identity caused collisions.
Why included: same subsystem + Git ancestor

Open thread
Verify identity behavior after remote URL changes.

[Build context pack] [Open all results]
```

The default is concise. Full transcript evidence opens on demand. Confidence, currentness, and contradiction state are never hidden behind color alone.

### Deck action: Build context pack

The action opens a small configuration sheet with task, scope, budget, source policy, and include/exclude controls. The preview shows token use, citations, warnings, and omissions before export or attachment.

### Global search

Agent history may appear as a separate result group when the provider is configured. It must not be blended indistinguishably with Workdeck projects, decks, units, or artifacts.

## Failure and Offline Behavior

The integration is optional. Workdeck remains fully usable when the service is absent, stopped, incompatible, or indexing.

- Health state is visible and non-blocking.
- Timeouts are short and cancellable.
- Search never runs on the UI thread.
- Partial or stale results are labeled.
- Frozen pack attachments remain readable offline.
- Workdeck does not silently start, install, upgrade, or repair the daemon.
- Schema incompatibility provides the installed and supported version range.
- Feedback queued offline is reviewable before retry.

## Privacy and Security

The provider may contain more sensitive material than the repository under review. Workdeck must not assume that a user who can open a deck can read every session.

Initial single-user integration requirements:

- connect only to an explicitly configured local endpoint or executable;
- authenticate local requests where the transport permits it;
- request the narrowest repository and unit scope possible;
- never log raw context or evidence in Workdeck diagnostics;
- never include context in telemetry or crash reports;
- show when remote inference or synchronization is enabled in the service;
- require explicit confirmation before freezing sensitive content into a Workdeck artifact;
- preserve citation redaction and access-denied states;
- expose deletion of Workdeck's cached or frozen copy separately from deletion at the source.

A future shared Workdeck requires permission-aware queries and per-result authorization from the service. Local filesystem access is not an adequate team permission model.

## Implementation Stages

### Stage 0: provider fixture

Define a deterministic fake provider and contract tests before selecting a real service. Cover healthy, unavailable, indexing, partial, stale, ambiguous-scope, redacted, contradicted, and version-incompatible responses.

### Stage 1: read-only CLI adapter

- provider configuration and health;
- scope resolution;
- search for a selected semantic unit;
- evidence expansion;
- no Workdeck writes beyond disposable cache and existing UI state.

This stage validates the product boundary without binding Workdeck to a daemon protocol.

### Stage 2: context-pack preview

- task and token-budget input;
- structured pack preview;
- inclusion explanations and citation navigation;
- copy/export without automatic agent launch;
- performance and cancellation instrumentation.

### Stage 3: frozen attachments and feedback

- content-addressed pack manifest and Markdown attachment;
- review-ledger event linking pack, deck, and checkpoint;
- accept, reject, pin, and supersede actions sent to the service;
- offline feedback queue with explicit retry.

### Stage 4: broader workflows

- correlate newly advanced worktree sources with likely agent sessions;
- context-aware global search;
- pass an approved pack to an agent through an explicit launch integration;
- evaluate team permissions only after the single-user model is trustworthy.

## Acceptance Criteria

The first useful integration is complete when:

- Workdeck works unchanged with no provider configured;
- a user can configure and health-check one local provider;
- a semantic unit can produce bounded, cited prior context;
- every displayed claim resolves to evidence or clearly reports that access is unavailable;
- ambiguous repository identity blocks automatic attachment;
- all requests are cancellable and stay off the render thread;
- a context pack respects its requested token budget;
- frozen packs reopen offline and retain integrity metadata;
- derived memories remain visibly distinct from primary evidence;
- no raw session content appears in normal Workdeck logs, telemetry, or repository files;
- provider errors, partial indexes, staleness, and policy omissions are visible;
- contract tests cover schema version negotiation and incompatible providers.

## Non-Goals

- Reimplementing session importers inside Workdeck.
- Copying the provider's full database into the Workdeck ledger.
- Automatically treating extracted memories as truth.
- Starting an agent with unreviewed context by default.
- Replacing repository documentation or Git history.
- Building hosted synchronization as part of the first integration.
- Making the context provider mandatory for Workdeck.

## Open Decisions

- Which implementation should be the first provider: a thin prototype, Callimachus, CASS plus a memory layer, or another compatible service?
- Is CLI JSON sufficient for the first experiment, or is an MCP/local API adapter worth defining immediately?
- Should frozen packs be a new deck-source kind or a normal content-addressed artifact with typed metadata?
- Which semantic-unit identifiers can safely cross the boundary, and which should remain Workdeck-private?
- How should pack redaction propagate when source evidence is later deleted or access-restricted?
- What latency target keeps inspector retrieval feeling native without encouraging shallow or stale caching?

## Next Validation

Build the Stage 0 fake provider and one vertical slice: select a semantic unit, retrieve three cited claims, inspect one exact evidence record, and compile a 2,000-token pack. Use it to test the boundary before changing the Workdeck schema or choosing a production context engine.
