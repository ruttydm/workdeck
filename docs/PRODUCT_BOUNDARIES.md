# Workdeck, Herder, and Aya

The three products collaborate through explicit contracts and keep separate sources of truth.

## Workdeck: repository and review plane

Workdeck is an advanced Git workbench. It owns repository discovery, branches, linked worktrees, commits, pull requests, CI evidence, diffs, semantic inspection, explicit Git operations, and durable review state.

Workdeck may display Herder session attribution, status, and links. It does not own the session process or infer repository truth from a session log.

## Herder: session execution plane

Herder owns agent-session identity and lifecycle, provider adapters, PTYs and processes, worktree allocation and leases, isolation, streaming output, retries, cancellation, and recovery.

A Herder worker may modify its leased worktree. Git remains the authority for what changed, and Workdeck is the place to inspect and review that result.

## Aya: control and experiment plane

Aya owns missions, intent, experiment definitions, policy and approval presentation, adaptive native panels, and cross-tool attention. Aya requests bounded work from Herder and consumes versioned evidence from Workdeck.

Aya does not implement a second session runner or a second diff/review engine.

## Integration contracts

```text
Aya -- work order / control request --> Herder
Aya <-- session events / results ------- Herder

Herder -- isolated agent writes ------> Git worktree
Workdeck <----------------------------> Git worktree

Workdeck -- review evidence ----------> Aya
Workdeck -- session attribution ------> Herder link
```

The contracts should use stable IDs and versioned JSON or typed protocols:

- `session_id`, `worktree_path`, `repository_identity`, and optional commit IDs connect Herder to Workdeck;
- review target, source revision, evidence revision, and review disposition connect Workdeck to Aya;
- mission ID, work-order ID, capability policy, and approval receipt connect Aya to Herder.

No product imports another product's private database.

## Hard rules

1. Workdeck never hosts a PTY or coding-agent process.
2. Herder is authoritative for session lifecycle and execution receipts.
3. Workdeck reads Git as repository truth and owns review semantics.
4. Aya owns intent and experiment state, not execution internals.
5. Every cross-product mutation is explicit, attributable, and idempotent where possible.
