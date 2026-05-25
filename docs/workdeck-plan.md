# Workdeck Product and Implementation Plan

Workdeck is a terminal-native sidecar for agentic coding. It is not an editor replacement. It is the persistent mental map pane beside Codex, an editor, lazygit, or a terminal multiplexer layout.

The product should run well in a narrow cmux pane while still scaling up to wider terminal layouts. It gives a structured overview of repo changes, file trees, previews, diffs, local issues, and agent work state.

## Product Positioning

Workdeck is the agentic coding workbench for the terminal side pane:

- Git awareness
- Repo map
- File and diff preview
- Local project management
- Agent session supervision

The MVP should stay local-first, fast, and predictable. It must never mutate Git state unless the user explicitly invokes an action.

## Primary Jobs

1. Show what changed, grouped by directory and intent.
2. Preview files and diffs quickly with strong syntax highlighting.
3. Track local issues, projects, and cycles in a Linear-like CLI UI.
4. Help humans supervise agentic coding sessions.
5. Make it easy to jump from overview to file, diff, issue, or agent session.

## Target UX

Workdeck is optimized for 40-80 column terminal panes.

Main layout modes:

- `Changes`: grouped dirty tree plus preview or diff
- `Files`: repo tree plus syntax preview
- `Issues`: Linear-like issues, projects, and cycles
- `Agents`: active and past agent sessions, plans, changed files, commands, tests, and handoff notes
- `Search`: fuzzy file, issue, symbol, and change search

Pane behavior:

- Narrow: single focused pane, preview toggled on demand
- Medium: tree or list plus preview
- Wide: tree or list plus preview

## Recommended Stack

Use Rust as the default implementation language.

- TUI: `ratatui` plus `crossterm`
- Git: `git2` for status and diffs; shell out to `git` for edge cases
- Syntax highlighting: `syntect` initially; tree-sitter later for richer symbols
- Search: `ignore`, `walkdir`, `nucleo` or `fuzzy-matcher`, plus optional ripgrep bridge
- Storage: file-based TOML and JSONL under `.agents/workdeck/`
- Config: TOML in `.agents/workdeck/config.toml`, with optional user-level fallback at `~/.config/workdeck/config.toml`
- Data: `.agents/workdeck/` by default when the repo has an `.agents` directory
- IPC/export: JSON and JSONL

Rust is the right default because Workdeck needs fast startup, a strong TUI ecosystem, responsive async work, and distributable single binaries.

## Repo Shape

Suggested repository layout:

```text
crates/
  workdeck-cli/
    src/
      main.rs
      app.rs
      config.rs
      git/
      tui/
      views/
        changes.rs
        files.rs
        issues.rs
        agents.rs
      store/
      search/
      syntax/
    tests/
    fixtures/
docs/
  workdeck-plan.md
```

The `store/` module should handle file-backed persistence and indexing. Do not introduce SQLite in the MVP.

## File-Based Persistence

Workdeck should use repo-native files, preferably inside the already existing `.agents` folder.

Default data directory resolution:

1. If `.agents/` exists in the repo root, use `.agents/workdeck/`.
2. If `.agents/` does not exist, create `.agents/workdeck/` when the user initializes Workdeck.
3. Use `.workdeck/` only as an explicit compatibility fallback if configured.
4. Use `~/.config/workdeck/config.toml` only for user-level defaults, never as the primary repo issue store.

Proposed data layout:

```text
.agents/
  workdeck/
    config.toml
    issues/
      WD-1.toml
      WD-2.toml
    projects.toml
    cycles.toml
    labels.toml
    agents/
      2026-05-24-agent-session.toml
    events.jsonl
    index/
      cache.json
```

The file store should be easy to review in Git and easy for agents to read and write. Workdeck should keep an in-memory index for speed and rebuild it from files on startup.

### Issue File Format

Use one TOML file per issue.

```toml
key = "WD-1"
title = "Add changes view"
description = "Render grouped dirty files with preview support."
status = "todo"
priority = "high"
project = "workdeck-mvp"
cycle = "mvp"
assignee = "rutger"
created_at = "2026-05-24T12:00:00Z"
updated_at = "2026-05-24T12:00:00Z"
due_at = ""

labels = ["mvp", "git"]
linked_files = ["crates/workdeck-cli/src/views/changes.rs"]
linked_commits = []
```

Issue keys should be stable human keys:

- `WD-1`
- `WD-2`
- `WD-3`

The next key can be computed by scanning existing issue files. For safety, write issue files atomically.

### Projects, Cycles, and Labels

Use shared TOML files for small reference data.

```toml
[[projects]]
id = "workdeck-mvp"
name = "Workdeck MVP"
description = "Initial terminal sidecar implementation."
status = "active"
created_at = "2026-05-24T12:00:00Z"
updated_at = "2026-05-24T12:00:00Z"

[[cycles]]
id = "mvp"
name = "MVP"
starts_at = "2026-05-24"
ends_at = ""
status = "active"

[[labels]]
id = "git"
name = "Git"
color = "green"
```

### Agent Sessions

Use one TOML file per agent session.

```toml
id = "2026-05-24-codex-workdeck-plan"
title = "Workdeck planning"
agent = "codex"
cwd = "/Users/rutger/Projects/workdeck"
status = "done"
started_at = "2026-05-24T12:00:00Z"
ended_at = "2026-05-24T12:30:00Z"
goal = "Create the Workdeck implementation plan."
summary = "Captured MVP scope and file-backed persistence model."

plan = [
  "Define product scope",
  "Define local data model",
  "Define build order"
]

commands_run = []
tests_run = []
handoff_notes = []

[[touched_files]]
path = "docs/workdeck-plan.md"
change_type = "added"
```

### Events

Append notable local events to JSONL:

```jsonl
{"kind":"issue_created","payload":{"key":"WD-1"},"created_at":"2026-05-24T12:00:00Z"}
{"kind":"agent_session_imported","payload":{"id":"2026-05-24-codex-workdeck-plan"},"created_at":"2026-05-24T12:30:00Z"}
```

The event log is for export, audit, and future integrations. It should not be required for normal startup if the TOML source files exist.

## Core Screens

### Changes

The Changes screen shows dirty Git state grouped by directory and intent.

Example:

```text
app/
  Http/
    Controllers/        3 files changed
resources/
  js/
    pages/              5 files changed
tests/                  2 files changed
```

Preview area:

- File diff
- Full file preview
- Staged and unstaged markers
- Additions and deletions
- Syntax-highlighted hunks

The tree should optimize for scanning. Use compact glyphs in the tree (`+`, `M`, `A`, `D`, `R`, `S`, `S+`) and compact churn tokens (`+12`, `-4`, `+12/-4`). Reserve verbose staged/unstaged labels for the footer, export, and status JSON.

Features:

- Group by directory
- Group by status: modified, added, deleted, renamed, untracked
- Toggle dirstat heat or weight
- Show staged versus unstaged
- Preview full file or diff
- Copy path
- Open in `$EDITOR`
- Stage or unstage file and hunk later, not MVP-critical

### Files

The Files screen shows the repo tree with `.gitignore` support.

Features:

- Repo tree
- File preview
- Syntax highlighting for common languages
- Binary, image, and archive metadata fallback
- Copy path
- Open in `$EDITOR`
- Optional hidden file toggle

### Issues

The Issues screen is a Linear-like local issue system.

Statuses:

- Inbox
- Backlog
- Todo
- In Progress
- In Review
- Done

Issue fields:

- `id`
- `key`
- `title`
- `description`
- `status`
- `priority`
- `project`
- `cycle`
- `labels`
- `assignee`
- `created_at`
- `updated_at`
- `due_at`
- `linked_files`
- `linked_commits`

Keyboard-first operations:

- `n`: create issue
- `e`: edit issue
- `s`: change status
- `p`: change priority
- `l`: toggle labels
- `A`: assign or unassign to the current local assignee
- `/`: search
- `Enter`: open issue
- `Space`: select

### Agents

The Agents screen tracks agent sessions manually at first, with JSONL and log imports later.

Session fields:

- `id`
- `title`
- `agent`
- `cwd`
- `status`
- `started_at`
- `ended_at`
- `goal`
- `plan`
- `touched_files`
- `commands_run`
- `tests_run`
- `summary`
- `handoff_notes`

Useful views:

- Active sessions
- Past sessions
- Files touched by agent
- Issues linked to session
- Latest test results
- Handoff notes

### Search

Search should be a fast overlay across:

- Files
- Dirty changes
- Issues
- Projects
- Cycles
- Labels
- Agent sessions
- Lightweight symbols for common source files
- Later: richer tree-sitter symbol indexing

Search results should support jumping directly to the relevant screen and selected entity.

## Keyboard Model

Use Vim-like defaults:

| Key | Action |
| --- | --- |
| `j` / `k` | Move tree selection, or scroll preview when preview is focused |
| `h` / `l` | Collapse or expand tree rows, or move between preview and tree focus |
| `Tab` / `Shift-Tab` | Switch top-level tab |
| `Enter` | Focus preview or open issue |
| `g` / `G` | Jump preview to top or bottom when preview is focused |
| `Esc` | Back, close, or clear |
| `/` | Search or filter |
| `?` | Help |
| `t` | Toggle preview |
| `g` | Group changes by directory or status |
| `w` | Toggle dirstat weight |
| `f` | Files |
| `c` | Changes |
| `G` | Git overview |
| `i` | Issues |
| `a` | Agents |
| `b` | Base branch selection placeholder in Git |
| `p` | PR refresh placeholder in Git, priority cycle in Issues |
| `l` | Toggle issue label in Issues |
| `A` | Assign or unassign issue in Issues |
| `Space` | Jump between issue and linked file |
| `o` | Open in editor |
| `y` | Copy path or id |
| `r` | Refresh |
| `q` | Quit |

Keybindings should be configurable through TOML.

Example config:

```toml
[ui]
theme = "auto" # auto, light, or dark
preview = true

[paths]
data_dir = ".agents/workdeck"

[git]
base_branch = ""
recent_commits = 30

[refresh]
auto = true
interval_ms = 1500
debounce_ms = 250

[keys]
quit = "q"
refresh = "r"
search = "/"
help = "?"
changes = "c"
git = "G"
files = "f"
issues = "i"
agents = "a"
toggle_preview = "t"
group_changes = "g"
toggle_dirstat = "w"
open_editor = "o"
copy = "y"
new_issue = "n"
edit_issue = "e"
status = "s"
priority = "p"
labels = "l"
assign = "A"
jump = "space"
link_file = "L"
base = "b"
pull_requests = "p"
```

## MVP Scope

Build this first:

1. Open a repo and render dirty Git changes grouped by directory.
2. Preview selected file or diff with syntax highlighting.
3. Toggle tree/list versus preview for narrow panes.
4. Local file-backed issue store with create, edit, status, and priority operations.
5. Link issues to files.
6. Fuzzy search across files, changes, and issues.
7. TOML config and keybindings.
8. Local-only Git overview tab for branch/upstream/base, recent commits, stashes, tags, and remotes.

Do not build these in the MVP:

- Cloud sync
- AI summaries
- Hunk staging
- Linear sync
- GitHub issue sync
- PR enrichment in Git tab, except planned placeholders
- Complex symbol indexing
- Multi-user collaboration

## Quality Bar

Startup target:

- Under 100ms in medium repos after caches are warm.
- Cold startup should render a useful shell quickly, then load heavier data async.

Responsiveness:

- Navigation must stay responsive in large Laravel and Node repos.
- Long operations must run async with loading states.
- Git and file scans must be superseded by newer refreshes; stale refresh results and errors must not overwrite the current snapshot.
- Respect `.gitignore`.

Safety:

- Never mutate Git state unless the user explicitly invokes an action.
- File-backed issue edits should use atomic writes.
- Preserve unknown top-level fields in issue and agent-session TOML so agents and future versions can add metadata safely.
- Avoid noisy rewrites of issue files.

## Architecture

### App Core

The app core owns:

- Current route/tab
- Focus model
- Selected item per view
- Preview state
- Search state
- Async job state
- Dirty data snapshots

The UI should render from immutable snapshots where possible. Background workers should send messages back into the app loop.

### Git Module

Responsibilities:

- Discover repo root
- Read status using `git2`
- Compute staged and unstaged file states
- Produce grouped directory tree
- Produce file diffs
- Fall back to shelling out to `git` for edge cases

The Git module must not stage, unstage, commit, checkout, reset, or mutate state during MVP read paths.

### Store Module

Responsibilities:

- Resolve `.agents/workdeck/`
- Read and write config
- Read and write issues
- Read projects, cycles, and labels
- Read and write agent sessions
- Append events
- Maintain in-memory indexes
- Write atomically

The store should expose typed structs and keep serialization details isolated.

### Search Module

Responsibilities:

- Build searchable records from files, changes, issues, and sessions
- Score fuzzy matches
- Return typed targets for navigation
- Refresh incrementally where practical

### Syntax Module

Responsibilities:

- Detect language from path and extension
- Highlight file previews
- Highlight diff hunks
- Provide plain fallback for unknown or large files

Large files should be truncated with clear metadata rather than blocking the UI.

## Build Order

### 1. Ratatui Shell

Deliver:

- Rust workspace
- `workdeck` binary
- Tabs
- Focus model
- Basic keymap
- Config loading
- Help overlay
- Narrow/medium/wide layout switching

Acceptance:

- `workdeck` opens in an empty repo without crashing.
- `q` quits.
- Tab switching works.
- Layout adapts below 80 columns.

### 2. Git Status Scanner

Deliver:

- Repo discovery
- Dirty file list
- Grouped directory tree
- Status badges
- Refresh key

Acceptance:

- Modified, added, deleted, renamed, and untracked files are shown.
- Grouping works by directory.
- Refresh reflects filesystem changes.
- Git state is not mutated.

### 3. Preview Pane

Deliver:

- File preview
- Diff preview
- Syntax highlighting
- Binary metadata fallback
- Preview toggle

Acceptance:

- Selecting a changed file shows its diff.
- Toggling preview works in narrow panes.
- Large files do not freeze navigation.

### 4. File Store and Issues View

Deliver:

- `.agents/workdeck/` initialization
- Issue TOML parser and writer
- Create issue
- Edit title and description
- Change status
- Change priority
- Issues list grouped by status

Acceptance:

- Issues are stored as reviewable TOML files.
- New keys are stable and sequential.
- Edits use atomic writes.
- Unknown data is not aggressively destroyed.

### 5. File and Issue Linking

Deliver:

- Link current selected file to an issue
- Show linked files on issue detail
- Jump from issue to file
- Jump from file to linked issues

Acceptance:

- Linking updates the issue TOML.
- Search and navigation recognize the relationship.

### 6. Search Overlay

Deliver:

- Fuzzy search across files, changes, and issues
- Typed result targets
- Jump-to-result behavior

Acceptance:

- `/` opens search.
- Typing filters quickly.
- `Enter` jumps to selected result.

### 7. Agents View

Deliver:

- Manual session files
- Agents list
- Session detail
- Touched files
- Commands and tests display
- Handoff notes

Acceptance:

- Agent sessions load from `.agents/workdeck/agents/*.toml`.
- Touched files can be opened or previewed.
- Sessions can be linked to issues later.

### 8. Polish and Hardening

Deliver:

- Loading states
- Error states
- Empty states
- Configurable keys
- Theme pass
- Performance pass on large repos

Acceptance:

- Workdeck remains usable in 40-column panes.
- No text overlap in common terminal sizes.
- Medium repo warm startup is under 100ms or the first useful paint is under 100ms with async loading.

## Testing Plan

Unit tests:

- Config loading and key parsing
- Store path resolution
- Issue key generation
- TOML round trips
- Git status grouping
- Search scoring targets

Integration tests:

- Fixture repos with dirty changes
- `.agents/workdeck/` issue fixtures
- Narrow and medium layout smoke tests
- Search jump behavior

Manual tests:

- Empty repo
- Medium Laravel repo
- Medium Node repo
- Repo with many untracked files
- Repo with binary files
- Repo without `.agents/`
- Repo with existing `.agents/`

## Initial Milestones

### Milestone 1: Navigable Shell

The app opens quickly, displays tabs, switches layouts, and handles keybindings.

### Milestone 2: Useful Changes View

The app shows dirty repo state grouped by directory with previewable diffs.

### Milestone 3: Local Issues

The app can create, edit, and organize local TOML-backed issues.

### Milestone 4: Connected Workbench

Files, changes, issues, and search are connected so the user can move from overview to detail quickly.

### Milestone 5: Agent Supervision

Agent sessions, touched files, test results, and handoff notes are visible in the side pane.

## Open Decisions

- Whether `.agents/workdeck/` should be auto-created on first run or only through `workdeck init`.
- Whether issue files should preserve original TOML formatting exactly or only preserve unknown fields.
- Whether events should be required for reconstructing history or remain best-effort audit/export data.
- Whether agent session imports should support Codex JSONL in the first public release or immediately after MVP.
- Whether to use `nucleo` or `fuzzy-matcher` for MVP search.

## Non-Goals

- Replacing an editor
- Replacing lazygit
- Replacing Linear
- Cloud sync
- Multi-user project management
- AI-generated planning or summaries in MVP
- Automatic Git mutations
