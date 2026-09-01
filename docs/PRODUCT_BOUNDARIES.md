# Workdeck, Herder, and Aya

The three products collaborate through explicit contracts and keep separate sources of truth.

## Workdeck: terminal repository and review plane

Workdeck is a TUI-only Git workbench. It owns terminal repository discovery, branches, linked worktrees, commits, diffs, local issues and handoffs, explicit Git operations, and durable review state.

The only shipped executable is `workdeck`. Its headless subcommands are part of the same terminal product and expose structured output for automation. Workdeck does not ship a desktop app, browser UI, embedded web server, or graphical component library.

Workdeck may display imported Herder session attribution, status, and links. It does not own the session process or infer repository truth from a session log.

## Herder: session execution plane

Herder owns agent-session identity and lifecycle, provider adapters, PTYs and processes, worktree allocation and leases, isolation, streaming output, retries, cancellation, and recovery.

A Herder worker may modify its leased worktree. Git remains the authority for what changed, and Workdeck is the terminal place to inspect and review that result.

## Aya: graphical control and experiment plane

Aya owns all graphical application surfaces, missions, intent, experiment definitions, policy and approval presentation, adaptive native panels, and cross-tool attention. It may request bounded work from Herder and consume repository evidence from Git or Workdeck's structured interfaces.

Aya does not replace Herder's session runner. Aya's graphical review experiments live in the private Aya repository, not in Workdeck.

## Integration contracts

```text
Aya -- work order / control request --> Herder
Aya <-- session events / results ------- Herder

Herder -- isolated agent writes ------> Git worktree
Workdeck <----------------------------> Git worktree

Workdeck -- structured evidence ------> Aya
Workdeck -- session attribution ------> Herder link
```

The contracts should use stable IDs and versioned JSON or typed protocols. No product imports another product's private database.

## Hard rules

1. Workdeck remains terminal-only and never hosts a PTY or coding-agent process.
2. Herder is authoritative for session lifecycle and execution receipts.
3. Git is authoritative for repository state; Workdeck owns its terminal workflow and repo-local review semantics.
4. Aya owns graphical presentation, intent, and experiment state.
5. Every cross-product mutation is explicit, attributable, and idempotent where possible.
