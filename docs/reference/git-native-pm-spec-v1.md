# Git-Native Project Management System
## Master Specification for Implementation (Codex / Claude / humans)

Version: 1.0  
Status: implementable  
Canonical name: `pm`  
On-disk root: `.project/`

This document is the source of truth for building a Linear/Redmine-like project manager whose **source of truth is files in Git**, designed first for human + coding-agent workflows, with a later path to multiplayer UI.

If a feature cannot be expressed as “a file change + a Git commit,” it does not belong in v1.

---

## 0. One-sentence pitch

A Linear-like issue tracker that lives as Markdown/YAML files in the repository, versioned with the code, operable offline via CLI, readable by agents with ordinary file tools, and projectable later into a board UI without ever making a database authoritative.

---

## 1. Goals and non-goals

### Goals

- Issues, projects, cycles, wiki, comments, time, and relations stored as plain files under `.project/`.
- Stable issue paths (IDs never depend on status folders).
- Merge-friendly layout for concurrent human/agent edits.
- First-class agent loop: `next` → `claim` → implement → `done`.
- Full audit trail via Git (author, time, diff of spec + code).
- Optional local SQLite **projection** for fast list/board/search (always rebuildable).
- A documented evolution path to shared web UI, presence, and notifications **without changing the file schema**.

### Non-goals (v1)

- Replacing GitHub/Linear as a hosted product.
- Real-time co-editing of issue bodies (CRDT).
- Fine-grained ACLs beyond “can push this repo.”
- Auto-commit of every keystroke.
- Two-way sync with Linear/Jira as the primary workflow.
- Storing issues as Git objects instead of files (`git-bug` style). Files in the worktree are required.

### Hard rule

**Files are the source of truth.** SQLite, UI state, presence, unread badges, and caches are disposable projections. `rm -rf .project/.index && pm reindex` must restore the UI.

---

## 2. Design principles

1. **Stable paths.** `PROJ-142` always lives at `.project/issues/PROJ-142/`. Status is a field, not a directory move.
2. **One concern per file.** Issue body, each comment, each time entry, each claim — separate files so two writers rarely touch the same blob.
3. **YAML frontmatter + Markdown body.** Machines parse the header; humans/agents edit the body.
4. **Git is the database and the bus.** Push/pull is multiplayer. Commits are the activity feed.
5. **One mutation library.** CLI, TUI, web, MCP, and agents all call the same write path. Nobody writes SQL as primary storage.
6. **Agent-native, human-readable.** JSON output for tools; Markdown for eyes.
7. **Branch policy is explicit.** Default: `.project/` is meaningful on `main` (and on a dedicated PM worktree/repo). Feature branches may carry issue edits, but claiming happens via push to the shared PM ref.

---

## 3. Repository layout

```text
<repo>/
├── src/                          # product code (example)
├── AGENTS.md                     # thin pointer to agent workflow
├── .project/
│   ├── config.yml                # project config (versioned)
│   ├── schema.yml                # field types, enums, required_when
│   ├── users.yml                 # known identities
│   ├── labels.yml                # allowed labels + colors/descriptions
│   ├── templates/
│   │   ├── bug.md
│   │   ├── feature.md
│   │   └── epic.md
│   ├── projects/
│   │   └── api-v2.yml
│   ├── cycles/
│   │   └── 2026-w37.yml
│   ├── views/
│   │   └── sprint-board.yml
│   ├── wiki/
│   │   └── home.md
│   ├── time/
│   │   └── 2026-09.jsonl         # append-only monthly log
│   ├── claims/                   # optional claim files (see §10)
│   ├── issues/
│   │   └── PROJ-01HXYZ.../       # or PROJ-0142/ depending on id_style
│   │       ├── item.md           # the ticket
│   │       ├── comments/
│   │       │   └── 20260908T153012Z-alice.md
│   │       ├── attachments/
│   │       ├── events/           # optional event log (see §8.2)
│   │       └── time.yml          # optional cached rollup
│   └── .index/                   # GITIGNORED projection
│       └── index.sqlite
├── .gitignore                    # must ignore .project/.index/ and local settings
└── .gitattributes                # merge drivers for item.md (phase 2+)
```

### `.gitignore` entries (required)

```gitignore
.project/.index/
.project/settings.local.yml
.project/.tmp/
```

`settings.local.yml` is per-machine (current user handle, editor, auto-stage). Never commit it.

---

## 4. Identifiers

Configured by `id_style` in `config.yml`.

### Option A — ULID (recommended default)

- Format: `{PREFIX}-{ULID}` e.g. `PROJ-01K3Q8R2N4M6P8S0TQVWXYZABC`
- Collision-safe across laptops and agents.
- Sortable by time.
- Directory name = full id.

### Option B — Sequential padded

- Format: `{PREFIX}-{NNNN}` e.g. `PROJ-0142`
- Requires a counter file `.project/issues/.counter` **and** a claim protocol; two agents minting at once will clash.
- Only use if humans demand short ids and a single writer (or a lock) exists.

### Rules

- IDs are immutable. Never rename the issue directory because the title changed.
- Title slug is **not** part of the path.
- References in prose: `PROJ-0142` or `[[PROJ-0142]]`.
- Filename for comments: `{ISO8601 basic UTC}-{author-slug}.md`  
  Example: `20260908T153012Z-alice.md`  
  If collision: append `-2`, `-3`.

---

## 5. File schemas

### 5.1 `config.yml`

```yaml
schema_version: 1
prefix: PROJ
id_style: ulid                  # ulid | sequential
auto_stage: true                # git add after mutations
commit_on_change: false         # true only for solo experiments
default_status: backlog
default_priority: medium
estimate_unit: points           # points | hours
claim_lease_minutes: 120

statuses:
  - backlog
  - todo
  - in_progress
  - in_review
  - done
  - canceled

transitions:
  backlog: [todo, canceled]
  todo: [in_progress, backlog, canceled]
  in_progress: [in_review, todo, canceled]
  in_review: [done, in_progress, canceled]
  done: [in_progress]            # reopen
  canceled: [backlog, todo]

types: [epic, feature, bug, task, chore]
priorities: [none, low, medium, high, urgent]
```

Unknown statuses/types must be rejected by `pm doctor` and by the mutation library.

### 5.2 `schema.yml`

Declares extra/custom fields (Redmine-style) and validation.

```yaml
schema_version: 1
issue:
  required: [id, title, type, status, created, updated]
  fields:
    severity:
      type: enum
      values: [s1, s2, s3, s4]
      required_when:
        type: bug
    customer:
      type: string
    estimate:
      type: number
      min: 0
```

Custom fields are stored under `custom:` in issue frontmatter.

### 5.3 `users.yml`

```yaml
users:
  - id: alice
    name: Alice Example
    emails: [alice@example.com]
    git_names: [Alice Example]
  - id: bob
    name: Bob Example
    emails: [bob@example.com]
  - id: agent-codex
    name: Codex Runner
    emails: [codex@agents.local]
    kind: agent
```

If the current Git `user.email` maps to a user, that is the default author. Otherwise use local settings or `--author`.

### 5.4 `labels.yml`

```yaml
labels:
  - id: bug
    color: "#ef4444"
  - id: auth
    color: "#6366f1"
  - id: agent
    description: Safe for unattended agent pickup
```

### 5.5 Project file — `.project/projects/api-v2.yml`

```yaml
id: api-v2
name: API v2
status: in_progress             # planned | in_progress | paused | done | canceled
lead: alice
target: 2026-10-01
description: |
  Replace the v1 auth gateway.
```

### 5.6 Cycle file — `.project/cycles/2026-w37.yml`

```yaml
id: 2026-w37
name: Week 37
starts: 2026-09-07
ends: 2026-09-13
status: active                  # planned | active | completed
goal: Ship SSO return_to + billing portal
```

Linear “cycle” ≈ sprint ≈ Redmine version.

### 5.7 View file — `.project/views/sprint-board.yml`

```yaml
id: sprint-board
name: Sprint board
group_by: status
filter:
  cycle: 2026-w37
  type: [bug, feature]
columns: [backlog, todo, in_progress, in_review, done]
sort: [priority_desc, updated_desc]
```

Views are queries, not stored boards. The UI groups live issue files.

### 5.8 Issue — `.project/issues/<ID>/item.md`

```markdown
---
id: PROJ-01K3Q8R2N4M6P8S0TQVWXYZABC
title: SSO redirect drops return_to on Safari
type: bug
status: todo
priority: high
project: api-v2
cycle: 2026-w37
parent: null
assignee: null
reporter: alice
reviewer: null
estimate: 3
due: 2026-09-12
labels: [auth, frontend]
blocked_by: []
blocks: []
related: []
claim:
  actor: null
  lease_until: null
  claim_id: null
custom:
  severity: s2
created: 2026-09-01T09:12:00Z
updated: 2026-09-01T09:12:00Z
closed: null
---

## Summary

After SSO, Safari lands on `/` instead of the original deep link.

## Acceptance

- [ ] `return_to` survives the IdP round-trip
- [ ] regression test in `auth.spec.ts`
- [ ] no change for already-logged-in users

## Notes

Repro: private window, `/settings/billing`, Sign in with Google.
```

#### Field semantics

| Field | Notes |
|---|---|
| `status` | Must be in `config.yml` statuses |
| `parent` | Epic / parent work package id or null |
| `blocked_by` / `blocks` / `related` | Arrays of issue ids. Mutation lib keeps `blocks`/`blocked_by` bidirectional |
| `claim` | Agent/human lease. See §10 |
| `closed` | ISO timestamp when entering `done` or `canceled`; null otherwise |
| `updated` | Always bumped on any mutation of this issue dir |

Title changes do **not** rename the directory.

### 5.9 Comment — `.project/issues/<ID>/comments/<ts>-<author>.md`

```markdown
---
id: 20260908T153012Z-alice
author: alice
created: 2026-09-08T15:30:12Z
in_reply_to: null
---

The cookie is `SameSite=Lax`. Safari drops it on the POST-back.
```

Never append comments to `item.md`. Concurrent commenters must not conflict.

### 5.10 Time log — `.project/time/YYYY-MM.jsonl`

One JSON object per line, append-only:

```json
{"ts":"2026-09-08T14:00:00Z","user":"alice","issue":"PROJ-01K3...","hours":1.5,"activity":"dev","note":"Safari cookie path"}
```

Optional per-issue cache `.project/issues/<ID>/time.yml` may be regenerated; the JSONL is authoritative.

### 5.11 Wiki

Normal Markdown under `.project/wiki/`.  
Wiki links: `[[PROJ-0142]]` resolves to the issue. Relative links for other wiki pages.

---

## 6. Relations

Allowed kinds: `blocked_by`, `blocks`, `related`, `parent`/`child` (via `parent` field).

Invariants the mutation library must maintain:

- If A `blocks` B, then B lists A in `blocked_by`, and vice versa.
- Cycles in `blocked_by` are errors (`pm doctor` fails).
- `pm next` never returns an issue that has an open (`not done|canceled`) blocker.
- Deleting/canceling an issue strips it from other issues’ relation arrays (or `pm doctor --fix` does).

---

## 7. Workflow / state machine

Default statuses: `backlog → todo → in_progress → in_review → done`, plus `canceled`.

Rules:

- `pm claim` sets `status: in_progress`, `assignee`, and `claim.*`.
- `pm done` requires: no open child issues (if any), and optionally all acceptance checkboxes checked if `config.require_acceptance_for_done: true`.
- `pm done` sets `closed` to now.
- `pm reopen` sets `status: in_progress` or `todo` and clears `closed`.
- Invalid transitions are hard errors.

Acceptance checkboxes are GitHub-style `- [ ]` / `- [x]` in the Markdown body. Parser must be indentation-tolerant.

---

## 8. Concurrency and merge strategy

### 8.1 v1 (required)

- Comments: one file each → natural Git merge.
- Time: JSONL append → natural merge (duplicate lines possible; de-dupe by `{ts,user,issue,note}`).
- `item.md` frontmatter: last-write-wins per **file** unless a merge driver exists. Document that two people should not edit title+status of the same issue without pulling first.
- `pm pull` (or plain `git pull`) before `claim` is mandatory in the agent protocol.

### 8.2 Phase 2 merge driver (recommended next)

`.gitattributes`:

```
.project/issues/**/item.md merge=pm-item
```

Custom merge driver parses both YAML maps, unions keys, conflicts only when the **same key** changed to different values on both sides. Body uses normal text merge.

### 8.3 Phase 3 event log (if status fights are common)

Do not mutate `item.md` for field changes. Append:

`.project/issues/<ID>/events/20260908T153012Z-alice-set-status.yml`

```yaml
op: set
field: status
from: todo
to: in_progress
actor: alice
ts: 2026-09-08T15:30:12Z
```

Reducer compiles events → snapshot `item.md`. Snapshot is a cache; events are truth. v1 should **not** implement this unless needed. Reserve the `events/` directory name.

---

## 9. Git conventions

### Commits

Do not auto-commit unless `commit_on_change: true`.

Suggested message format when the CLI/server does commit:

```text
pm(<ID>): <verb> <summary>

pm(PROJ-0142): claim alice
pm(PROJ-0142): comment — SameSite note
pm(PROJ-0142): status todo → in_review
```

`git add` scope: only files the mutation touched. Never `git add -A` from the CLI.

### Branch policy (choose one; put it in config)

**Policy `main-canonical` (default recommendation for teams):**

- Shared source of truth for `.project/` is `origin/main` (or a sibling repo `org/app-pm`).
- Agents fetch `origin` before claim.
- Implementation happens on `feat/...` branches; issue claim/status updates are committed and pushed to the PM canonical ref.
- Optional: sibling repo so code PRs stay clean.

**Policy `travels-with-branch`:**

- Issues on a feature branch are visible only on that branch.
- Useful for “this spike’s plan.”
- Forbidden as the only mode for a multi-agent team (tickets disappear on checkout).

### Worktree option

```text
git worktree add ../app-pm pm
```

`pm` branch / worktree holds only `.project/` if using an orphan branch. Document in README.

### Hooks (phase 1+)

- `pre-commit`: run `pm doctor --staged` (schema, ids, relation symmetry).
- `post-merge` / `post-checkout`: `pm reindex` if index exists.

---

## 10. Agent protocol (normative)

This is the part agents must follow. Put a short version in `AGENTS.md` and the full version in `.project/AGENT-PROTOCOL.md` (copy of this section).

### 10.1 Cold start

1. Read `.project/config.yml` and this protocol.
2. Identify self: `--author` or mapped git email or `settings.local.yml` `user`.
3. `git fetch` + rebase/merge the PM canonical ref.
4. `pm next --json` (or equivalent library call).
5. If empty, stop. Do not invent work unless asked.

### 10.2 Pickup loop

```text
pm next --json
pm claim <ID> --actor <me> --lease-minutes 120
# implement against item.md acceptance criteria
# run tests named in the issue
pm comment <ID> --body "..."        # what changed, SHA if any
pm done <ID>                        # only if acceptance met
# or pm release <ID> if blocked / failed
```

### 10.3 Claim rules

A claim is valid if:

- `claim.actor` is set
- `claim.lease_until` is in the future (UTC)
- `claim.claim_id` is unique: `{actor}-{utc}-{rand4}`

`pm claim` algorithm:

1. Fetch.
2. Read `item.md`.
3. If valid foreign claim → refuse (`exit 3`).
4. If own expired claim → renew.
5. Write `claim`, `assignee`, `status=in_progress`, `updated`.
6. Stage (and push if `--push`).
7. If push rejected, pull --rebase and retry once; if still claimed by other, abort.

`pm release` clears `claim.*`, sets status back to `todo` unless `--keep-status`.

`pm next` filters:

- `status` in `{backlog, todo}` OR (`in_progress` with expired claim)
- no open blockers
- not canceled/done
- optional `--label agent` so humans can mark agent-safe work
- highest `priority`, then oldest `created`

### 10.4 Parallel agents

- One issue → one valid claim.
- Do not edit another agent’s in-lease issue except `pm comment`.
- Prefer separate issues (children) over two agents on one `item.md`.
- After fetch, list claims: `pm claims --json`.

Optional stronger lock (phase 2): push branch `claim/<ID>` containing `.project/claims/<ID>.md`. Discovery: `git ls-remote --heads origin 'claim/*'`. v1 file-field claims are enough if everyone fetches.

### 10.5 What agents must write back

Before `done`:

- Acceptance checkboxes that are actually done.
- A comment with: summary, test command + result, commit SHAs.
- Do **not** delete requirements to make the ticket look finished.
- Do **not** mark `done` if tests named in the issue were not run.

### 10.6 JSON shapes

`pm next --json`:

```json
{
  "id": "PROJ-01K3...",
  "title": "SSO redirect drops return_to on Safari",
  "priority": "high",
  "path": ".project/issues/PROJ-01K3.../item.md",
  "blocked_by": [],
  "acceptance_open": 3
}
```

`pm show <ID> --json` returns frontmatter + body + comments array + derived `blocked`.

Exit codes:

| Code | Meaning |
|---|---|
| 0 | ok |
| 2 | validation / doctor error |
| 3 | claim conflict |
| 4 | not found |
| 5 | invalid transition |
| 6 | git error |

---

## 11. CLI surface (v1)

Binary name: `pm`.

```text
pm init [--prefix PROJ] [--id-style ulid]
pm doctor [--fix] [--staged]
pm reindex

pm new --title "..." --type bug [--priority high] [--label auth] [--body-file -]
pm show <ID> [--format text|json|md]
pm list [--status S] [--assignee A] [--cycle C] [--label L] [--format table|json]
pm board [--view sprint-board] [--format text|json]

pm set <ID> <field> <value>
pm edit <ID>                    # $EDITOR on item.md
pm comment <ID> [--body "..."] [--body-file -]
pm relate <ID> blocks|blocked_by|related <OTHER>
pm unrelate <ID> ...

pm next [--label agent] [--format json]
pm claim <ID> [--actor X] [--lease-minutes N] [--push]
pm release <ID> [--keep-status]
pm done <ID> [--require-acceptance]
pm reopen <ID>
pm cancel <ID> --reason "..."

pm cycle list|show|new
pm project list|show|new
pm time log --issue ID --hours 1.5 --activity dev --note "..."
pm time report [--issue ID] [--user U] [--month 2026-09]

pm wiki path                  # print wiki root
```

`pm init` creates the directory tree, default config/schema/users, `wiki/home.md`, and prints next steps.

Implementation language: whatever the implementing agent prefers (Go, Rust, TypeScript). Must be a single local binary or `npx`-able CLI with no required server.

---

## 12. Mutation library (internal API)

All writers use one package, conceptually:

```text
Store
  LoadIssue(id) -> Issue
  SaveIssue(issue, opts)        # writes item.md, bumps updated, optional git add
  AddComment(id, comment)
  ListIssues(filter) -> []IssueExcerpt
  Claim(id, actor, lease) -> Result
  RebuildIndex()
```

`SaveIssue` must:

- validate schema
- rewrite bidirectional relations
- set `updated`
- not rewrite unrelated comments
- write atomically (temp file + rename)

Never pretty-print-reorder unrelated YAML keys in a way that explodes diffs. Use a stable field order matching the template in §5.8.

---

## 13. Projection index (v1 optional but recommended)

Path: `.project/.index/index.sqlite` (gitignored).

Tables (minimum):

```sql
issues (
  id TEXT PRIMARY KEY,
  path TEXT,
  title TEXT,
  type TEXT,
  status TEXT,
  priority TEXT,
  assignee TEXT,
  reporter TEXT,
  project TEXT,
  cycle TEXT,
  parent TEXT,
  created TEXT,
  updated TEXT,
  closed TEXT,
  labels_json TEXT,
  frontmatter_json TEXT
);
comments (
  issue_id TEXT,
  path TEXT,
  author TEXT,
  created TEXT
);
relations (
  from_id TEXT,
  to_id TEXT,
  kind TEXT
);
```

Plus FTS5 on title + body if the runtime allows.

Rebuild: walk `.project/issues/*/item.md`.  
Invalidate: filesystem watcher in UI process; CLI can rebuild on each `list` if cheap, or mtime-check.

If index and files disagree, files win.

---

## 14. Human UX (v1)

- CLI is enough.
- `pm board` prints columns as text.
- Humans may edit `item.md` in any editor. After hand-edits they should run `pm doctor`.
- Optional later: TUI, VS Code viewer, localhost web board.

Do not block v1 on a GUI.

---

## 15. Evolution roadmap (do not implement all now)

Build in this order. Each phase keeps the same file schema.

### Phase 0 — merge-safe files + schema  ← **implement this first**

Layout, `item.md`, comments, config, doctor, init.

### Phase 1 — CLI + agent protocol

`new/list/show/set/comment/next/claim/done`, relation sync, JSON output.

### Phase 2 — index + local board

SQLite projector, `pm board`, optional `pm serve --bind 127.0.0.1` read/write UI that calls the mutation library and writes files.

### Phase 3 — async multiplayer via Git

Document sibling-repo or `main-canonical`. Frontmatter merge driver. `pre-commit` doctor. Notifications via `post-receive` (email/Slack) parsing new commits under `.project/`.

### Phase 4 — shared `pm-server`

One process, one worktree, many browsers.

Write path: UI → API → mutation lib → commit as authenticated user → push → reindex → websocket `issue.updated`.

Server is a Git client with a UI, not an application database.

Identity: map OIDC/GitHub user → `users.yml` id → `GIT_AUTHOR_EMAIL`.

### Phase 5 — live layer (ephemeral)

Presence, typing indicators, unread inbox. Store in memory/Redis or gitignored user state. Never the issue body.

Live co-edit of descriptions only if required; CRDT the **body only**, snapshot to Git on blur. Frontmatter stays per-field last-write-wins.

### Phase 6 — optional bridges

Frontmatter `external: { github: 123 }` and explicit `pm pull-gh` / `pm push-gh`. Files remain canonical internally. Do not dual-write forever.

### Forbidden evolution

Postgres/Linear as source of truth with a nightly Markdown dump.

---

## 16. Multiplayer / UI notes for later implementers

- One writer process per worktree. Many tabs → one server.
- Browser drafts must become files (`*.draft.md`) or a user branch, not IndexedDB-only.
- Unread mailboxes may live outside Git.
- Permissions: start with “can push.” Later, server enforces transitions before commit.
- Hosting: each dev runs UI locally **or** one always-on box with the PM worktree.

---

## 17. Why this exists in an agentic world (context for implementers)

Do not optimize for a pretty board in v1. Optimize for:

1. Agents can `read_file` the spec without OAuth.
2. Same diff contains spec change + code change.
3. `next/claim/done` is a local state machine.
4. Parallel agents lock via Git, not a SaaS mutex.
5. History is `git log`, not a vendor event stream.
6. Worktrees can fork the plan.
7. CI can fail a PR if `done` was set with open acceptance boxes.

Keep `AGENTS.md` thin: point at `.project/AGENT-PROTOCOL.md` and `pm next`. Do not dump the whole backlog into the system prompt.

---

## 18. `AGENTS.md` snippet (install in target repos)

```markdown
## Project management

Work items live in `.project/issues/*/item.md` (YAML frontmatter + Markdown).

Before starting work:
1. `pm next --json` (or read open issues under `.project/issues/`)
2. `pm claim <ID> --push`
3. Implement the acceptance checkboxes in `item.md`
4. Comment what you did and the test command
5. `pm done <ID>` only if acceptance is actually met

Never invent a parallel todo list. Never delete acceptance criteria to close a ticket.
If `pm` is unavailable, edit the files directly and keep relations bidirectional.
Full protocol: `.project/AGENT-PROTOCOL.md`
```

---

## 19. Testing requirements

Implement as soon as CLI exists:

- parse/serialize round-trip does not scramble field order or body
- `claim` conflict returns exit 3 and does not clobber foreign lease
- bidirectional `blocks` / `blocked_by`
- `doctor` catches unknown status, missing id, relation dangling ref
- sequential id style (if implemented) does not reuse ids
- comment files from two authors merge in a test repo without conflict
- `done` refused on open blockers
- `reindex` after deleting sqlite matches `list --json`

Use a temp Git repo in tests. Do not touch the user’s real repo.

---

## 20. Implementation plan for the coding agent building `pm`

Do this in order. Commit after each step.

1. Create module skeleton and `pm init` that writes default `.project/**`.
2. Issue model + YAML/Markdown parser/serializer with stable key order.
3. `new`, `show`, `list`, `set`, `edit`.
4. Comments + time log.
5. Relations + doctor.
6. `next`, `claim`, `release`, `done`, `reopen`, `cancel` with exit codes.
7. Git helpers: detect repo root, optional `git add` of touched files, never `add -A`.
8. JSON output on all read commands.
9. `--push` on claim/done (fetch + push, rebase once on reject).
10. `reindex` + use index in `list`/`board` when present.
11. Write `.project/AGENT-PROTOCOL.md` and root `AGENTS.md` snippet into `pm init`.
12. README with layout, protocol, and roadmap phases 2–6 as documentation only.

Out of scope unless explicitly requested after v1 works: web UI, merge driver, MCP server, GitHub bridge.

---

## 21. Default templates

### `templates/bug.md`

```markdown
---
type: bug
priority: high
---

## Summary

## Repro

1.

## Expected

## Actual

## Acceptance

- [ ]
```

### `templates/feature.md`

```markdown
---
type: feature
priority: medium
---

## Summary

## Why

## Acceptance

- [ ]
```

### `templates/epic.md`

```markdown
---
type: epic
status: backlog
---

## Intent

## Children

(created as separate issues with `parent:` set)

## Acceptance
```

---

## 22. Definition of done for v1 of this system

A stranger (or Codex) can:

1. `pm init` in a git repo.
2. `pm new --title "Fix login" --type bug`.
3. Open the created `item.md` and see valid frontmatter.
4. `pm claim <ID>` and see assignee + lease + `in_progress`.
5. Add a comment file via `pm comment`.
6. `pm relate A blocks B` and see both files updated.
7. `pm done <ID>` and see `closed` timestamp.
8. `pm doctor` exit 0.
9. All of the above visible in `git status` as explicit files under `.project/`.
10. A second clone after commit+push can `pm list` and see the same issues.

If those ten work, the idea is real. Everything else is a projection.

---

## 23. Glossary

| Term | Meaning |
|---|---|
| Issue / work item | One directory under `.project/issues/` |
| Projection | Disposable index/UI derived from files |
| Claim | Time-bounded lease preventing double pickup |
| Cycle | Time box (sprint / Linear cycle) |
| Canonical PM ref | Branch/repo that team treats as shared backlog |
| Mutation library | Sole code path that writes issue files |

---

## 24. Explicit decisions already made

Do not reopen these without a human:

- Files in `.project/`, not Git-notes / git-bug objects.
- Status is a field, not `open/` `closed/` folders.
- Comments are separate files.
- ULID ids by default.
- No auto-commit by default.
- Agents must fetch before claim.
- UI comes after CLI.
- Database never becomes source of truth.
