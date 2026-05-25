# Files UX and Headless CLI Plan

Workdeck has two equally important product surfaces:

1. The TUI sidecar for a compact mental model while coding.
2. The headless CLI for scripts, agents, hooks, and non-interactive project operations.

This plan fixes the Files browser first, then rounds out the CLI so every local Workdeck object can be managed without opening the TUI.

## Part 1: Files UX

### Problem

The current Files view is an expandable full repo tree. That works in medium and wide terminals, but it is weak in the 40-80 column sidecar case:

- Deep paths consume too much width.
- Expanding multiple folders creates visual noise.
- It is hard to build a simple "where am I?" mental model.
- Navigation feels like a tree widget rather than a file browser.

Changes can stay tree-first because the changed set is usually small. Files needs a browser model because the repo set can be huge.

### Target Model

Use two Files navigation modes:

- Narrow: drill-down browser.
- Medium and wide: tree/list plus preview.

The narrow Files view should look like this:

```text
Files > crates > workdeck-cli > src

../
app.rs
config.rs
git/
search/
store/
syntax/
tui/
views/
```

The user should feel like they are moving through folders, not managing an expanded outline.

### Navigation

```text
f             Files tab
j/k           move within current folder
l / Enter     enter folder, or preview file
h / Esc       parent folder, or return from preview to browser
Backspace     parent folder
g/G           top/bottom
/             search files/tasks/changes
t             toggle preview availability
o             open selected file in $EDITOR
y             copy selected path
n             create issue from selected file
L             link selected file to selected issue
```

### State Changes

Add Files-specific browser state to `App`:

```rust
files_cwd: PathBuf
selected_file_entry: usize
file_browser_scroll: usize
```

Keep the existing full tree state for medium/wide:

```rust
selected_file
selected_file_row
collapsed_file_dirs
```

Rules:

- `files_cwd` is repo-relative and starts at `""`.
- `../` appears when `files_cwd` is not repo root.
- Directories sort before files.
- Hidden and ignored files remain excluded through the existing ignore-backed scanner.
- Selection is clamped after refresh or folder changes.
- Entering a folder should reset selection to the first child.
- Returning to the parent should keep the folder just exited selected.

### Rendering

Narrow Files:

- Header line is a breadcrumb: `Files > crates > workdeck-cli > src`.
- List shows only one directory level.
- Directories end with `/`.
- Files can show small metadata only when useful and cheap.
- No inline full paths in narrow mode unless the item name would be ambiguous.

Medium/wide Files:

- Keep the current tree plus preview layout.
- Reuse the same selection target as narrow mode where practical.
- If the selected file came from search or task jump, reveal its parent path.

### Preview Behavior

Narrow mode should match the fixed Changes behavior:

- Browser is the default.
- Preview renders full-screen only when focus is `Preview`.
- `l` or `Enter` on a file moves to preview focus.
- `h` returns to the browser.
- Resizing from medium/wide to narrow must not unexpectedly replace the browser with preview.

### Search and Jump Behavior

When a file search result is accepted:

1. Set `files_cwd` to the file parent.
2. Select the file in that directory.
3. Switch to Files.
4. Keep focus on browser unless the user explicitly requested preview.

When jumping from an issue to a file:

1. Same parent-folder reveal behavior.
2. Status line should say `file path/to/file`.
3. Preview remains available but not forced in narrow mode.

### Tests

Add focused renderer and app-state tests:

- Root Files browser shows direct children only.
- `l` enters a selected folder.
- `h` and Backspace return to parent.
- Returning to parent selects the folder just exited.
- Narrow Files defaults to browser even when preview is enabled.
- Enter on file focuses preview.
- Search result reveal sets `files_cwd` and selection.
- Ignored files remain absent.

### Implementation Order

1. Add file browser state and folder-entry derivation.
2. Add navigation methods: enter folder, parent folder, reveal file in browser.
3. Route Files key handling through browser-aware methods.
4. Render narrow Files as drill-down browser.
5. Keep medium/wide tree behavior working.
6. Add tests for resize and search/jump reveal behavior.
7. Update README key docs and UX audit notes.

## Part 2: Full Headless CLI Support

### Principle

Anything useful in the TUI should be possible from `workdeck` without opening the TUI. The TUI is an interface over the same store and repo scanners; it should not be the only way to mutate Workdeck data.

Every headless command should support:

- Human-readable default output.
- `--json` for structured output.
- Stable exit codes.
- No Git mutation unless the command name explicitly performs a mutation.
- Repo-local `.agents/workdeck/` storage by default.

### Current Headless Surface

Already present:

- `workdeck --init`
- `workdeck --status-json`
- `workdeck doctor [--json]`
- `workdeck export [--jsonl]`
- `workdeck issue list/create/update/link/show [--json]`
- `workdeck project list/save [--json]`
- `workdeck cycle list/save [--json]`
- `workdeck label list/save [--json]`
- `workdeck agent list/record/show/import [--json]`

This is a good start, but it is not yet a complete CLI product.

### CLI Shape

Move toward consistent nouns and verbs:

```text
workdeck status [--json]
workdeck files list [PATH] [--json]
workdeck files show PATH [--json]
workdeck changes list [--group directory|status] [--json]
workdeck changes diff PATH [--json]

workdeck issue list [--status todo] [--project ID] [--json]
workdeck issue create TITLE [fields...] [--json]
workdeck issue update KEY [fields...] [--json]
workdeck issue close KEY [--json]
workdeck issue reopen KEY [--json]
workdeck issue delete KEY [--yes] [--json]
workdeck issue link-file KEY PATH [--json]
workdeck issue unlink-file KEY PATH [--json]
workdeck issue link-commit KEY SHA [--json]
workdeck issue unlink-commit KEY SHA [--json]

workdeck project list [--json]
workdeck project save NAME [fields...] [--json]
workdeck project show ID [--json]
workdeck project delete ID [--yes] [--json]

workdeck cycle list [--json]
workdeck cycle save NAME [fields...] [--json]
workdeck cycle show ID [--json]
workdeck cycle delete ID [--yes] [--json]

workdeck label list [--json]
workdeck label save NAME [fields...] [--json]
workdeck label show ID [--json]
workdeck label delete ID [--yes] [--json]

workdeck agent list [--json]
workdeck agent record TITLE [fields...] [--json]
workdeck agent show ID [--json]
workdeck agent update ID [fields...] [--json]
workdeck agent finish ID [--summary TEXT] [--json]
workdeck agent delete ID [--yes] [--json]
workdeck agent import PATH [--json]

workdeck search QUERY [--target files,changes,issues,agents] [--json]
```

Keep old aliases where they already exist:

- `--status-json` can remain as an alias for `status --json`.
- `issue link` can remain as an alias for `issue link-file`.

### Output Contract

Use a consistent envelope for `--json` on commands that are actions:

```json
{
  "ok": true,
  "kind": "issue",
  "action": "update",
  "data": {}
}
```

For list commands:

```json
{
  "ok": true,
  "kind": "issue_list",
  "data": []
}
```

For errors:

```json
{
  "ok": false,
  "error": {
    "code": "issue_not_found",
    "message": "issue WD-12 does not exist"
  }
}
```

Do not break plain output. Human output should stay compact and script-friendly.

### Exit Codes

```text
0   success
1   general failure
2   invalid arguments or validation error
3   not found
4   conflict or duplicate
5   config/store parse error
```

This matters for agents and shell scripts.

### Issue CLI Completion

Missing commands to add:

- `issue close KEY`
- `issue reopen KEY`
- `issue delete KEY --yes`
- `issue unlink-file KEY PATH`
- `issue link-commit KEY SHA`
- `issue unlink-commit KEY SHA`
- `issue assign KEY USER`
- `issue unassign KEY`
- `issue label add KEY LABEL`
- `issue label remove KEY LABEL`
- `issue move KEY --status STATUS`
- Filtering for `issue list` by status, priority, project, cycle, label, assignee, due date.

Also add `--stdin-json` or `issue create --from-json PATH|-` for agents creating richer issues without fragile shell quoting.

### Project, Cycle, and Label Completion

Missing commands:

- `show`
- `delete --yes`
- filtered `list`

Add validation:

- Deleting a project or cycle should not silently orphan issues unless `--force` is passed.
- Deleting a label should remove it from issue label lists or fail with a clear conflict.

### Agent CLI Completion

Missing commands:

- `agent update ID`
- `agent finish ID`
- `agent delete ID --yes`
- `agent append-plan ID TEXT`
- `agent add-file ID PATH [--change-type modified]`
- `agent add-command ID TEXT`
- `agent add-test ID TEXT`
- `agent add-note ID TEXT`

This lets a running agent session append state incrementally without rewriting a full TOML file manually.

### Repo and File CLI

Add commands that expose the same read-only data as the TUI:

- `status --json`
- `changes list --json`
- `changes diff PATH`
- `files list [PATH] --json`
- `files show PATH`
- `search QUERY --json`

Rules:

- `files list` respects `.gitignore`.
- `files show` uses the same text/binary/metadata preview path as the TUI.
- `changes diff` uses the same staged/unstaged diff preview logic.
- These commands never create `.agents/workdeck/`.

### Config CLI

Add:

- `config path`
- `config show [--json]`
- `config init`
- `config validate`
- `config get KEY`
- `config set KEY VALUE`

Be conservative with `config set`; validate immediately and preserve unknown TOML fields.

### Import and Export

Extend import/export for backup and automation:

- `export --json`
- `export --jsonl`
- `import PATH [--merge|--replace]`
- `import --dry-run`
- `events list [--json]`

All import paths should validate first, then write.

### Tests

Add integration tests for every CLI command:

- Human output smoke.
- `--json` parseability and shape.
- Store mutation result.
- No mutation for read-only commands.
- Validation failures with stable non-zero exits.
- `.agents` not created by read-only commands.

Add fixture-driven tests:

- Empty store.
- Store with issues/projects/cycles/labels.
- Corrupt issue file.
- Agent session import JSON and JSONL.
- Dirty Git repo with staged, unstaged, deleted, renamed, untracked files.

### Implementation Order

1. Files drill-down UX in narrow mode.
2. Normalize `status --json` while keeping `--status-json`.
3. Add `changes` and `files` read-only CLI commands.
4. Add issue subcommands for close/reopen/delete/link/unlink/filtering.
5. Add project/cycle/label show/delete/filtering.
6. Add agent update/finish/append commands.
7. Add search CLI.
8. Add config CLI.
9. Add import dry-run and events list.
10. Lock JSON output contracts and exit codes with integration tests.

## MVP Acceptance

Files UX is ready when:

- A 44-column terminal can navigate repo folders without horizontal path noise.
- Tree remains default in narrow mode unless preview is focused.
- Search and issue jumps reveal files in their parent folder.

Headless CLI is ready when:

- A script can create, update, link, close, reopen, list, filter, and export issues without TUI interaction.
- Agents can record and append session state incrementally.
- Repo status, changed files, diffs, file previews, and search are available as JSON.
- Read-only commands never create `.agents/workdeck/`.
- Every command has integration coverage for plain and JSON output.
