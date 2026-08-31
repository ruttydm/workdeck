# Deterministic fixture catalog

Workdeck never uses a portfolio repository for a state-changing test. The release matrix creates all mutable repositories, worktrees, provider stubs, catalogs, artifacts, and preference files beneath guarded temporary roots.

## Daily-driver scale fixture

`scripts/scale-fixture-gates.sh` constructs the production-shape catalog from scratch:

| Dimension | Required value |
| --- | ---: |
| Projects | 74 |
| Repository identities | 75 |
| Worktrees | 109 |
| Available worktrees | 105 |
| Intentionally empty projects | 7 |
| Retained unavailable and prunable worktrees | 4 |

The generator creates 74 projects, assigns 75 independent Git common directories across the 67 non-empty projects, adds 34 linked worktrees, removes exactly four linked directories to create retained prunable history, refreshes twice, and proves identity counts remain stable. It then checks SQLite integrity and foreign keys and runs the full bounded inbox scan.

## Functional fixture

`scripts/feature-matrix.sh` creates an isolated polyglot review with commit-range, Markdown, dirty-worktree, durable-review, and HTML-artifact sources. It verifies repeated discovery, checkpoint capture, marks, incremental inbox caching, loopback artifact serving, helper shutdown, and catalog integrity.

## Unit and integration fixtures

- `workdeck-git` covers linear, branch, two-parent merge, octopus merge, refs, tags, stashes, detached history, bounded search, cancellation, and timeouts.
- `workdeck-domain`, `workdeck-analysis`, and `workdeck-core` cover unchanged, modified, moved, formatting-only, ambiguous, removed, and new semantic units plus durable carry-forward.
- `workdeck-github` uses a local executable provider double for paginated PRs, checks, runs, jobs, logs, artifacts, sanitization, body limits, and atomic downloads.
- `workdeck-artifacts` creates safe and hostile ZIPs for traversal, symlink, size, origin, CSP, and shutdown checks.
- UI state tests use immutable snapshots and temporary preference roots; they do not launch Git mutations.

Every generated root has a narrow cleanup guard. A cleanup path outside its exact temporary prefix is rejected.
